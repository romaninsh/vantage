//! Generic augmentation: enrich a master Vista's rows from a *second* Vista,
//! loaded one row at a time and merged on top.
//!
//! The master is listed; for each visible row an [`Augmentation`] resolves a
//! detail Vista (from the [`VistaCatalog`]), narrows it for that row, fetches a
//! record, and merges chosen columns onto the master row. The detail source may
//! be the same Vista as the master (today's cmd two-pass) or an entirely
//! different backend (REST master enriched by a cmd script, or vice versa).
//!
//! This is the runtime, closure-based form. [`AugmentSpec`] is the serde/YAML
//! form; [`lower_augment`] turns one into the other (the only place Rhai is
//! touched). A consumer can also build [`Augmentation`] by hand — `Source::Build`
//! and `Fetch::Custom` take plain Rust closures.

mod lower;
mod spec;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{ReferenceKind, Vista};
use vantage_vista_factory::{Relation, VistaCatalog};

pub use lower::lower_augment;
pub use spec::{AugmentSpec, FetchSpec, SetOp, SourceSpec};

/// Narrow a freshly resolved `base` detail Vista for one master `row`. Produced
/// by hand or by Rhai (via `vantage_vista::augment_source_closure`, rhai feature).
pub type BuildFn = Arc<dyn Fn(&Record<CborValue>, Vista) -> Result<Vista> + Send + Sync>;

/// Pull records from a narrowed detail Vista.
pub type FetchFn = Arc<
    dyn Fn(Vista) -> Pin<Box<dyn Future<Output = Result<Vec<Record<CborValue>>>> + Send>>
        + Send
        + Sync,
>;

/// How a master row selects its detail record(s).
pub enum Source {
    /// `master.id → detail.id`.
    Id,
    /// `master[from] → detail[to | detail.id]`.
    Column { from: String, to: Option<String> },
    /// Arbitrary narrowing of the base detail Vista from the whole row.
    /// Per-row only — a built Vista can't be coalesced into a set query.
    Build(BuildFn),
}

/// How the narrowed detail Vista is read.
pub enum Fetch {
    /// One detail record per master row.
    PerRow,
    /// One set query across the window's distinct keys (phase 2).
    Batched { op: SetOp },
    /// Caller-supplied fetch.
    Custom(FetchFn),
}

/// Which detail columns land on the master row.
pub struct MergeRule {
    /// Columns to lift. Empty = lift all detail columns.
    pub columns: Vec<String>,
}

impl MergeRule {
    fn wants(&self, key: &str) -> bool {
        self.columns.is_empty() || self.columns.iter().any(|c| c == key)
    }

    /// Merge `detail`'s columns into `dest`. Detail values win on a name clash —
    /// the detail record is the authoritative hydration of the row, so it
    /// overwrites the cheap list-pass value (and adds its new columns).
    pub fn apply(&self, dest: &mut Record<CborValue>, detail: &Record<CborValue>) {
        for (k, v) in detail {
            if self.wants(k) {
                dest.insert(k.clone(), v.clone());
            }
        }
    }
}

/// One declared augmentation in runtime form.
pub struct Augmentation {
    /// Catalog name of the detail model.
    pub table: String,
    pub source: Source,
    pub fetch: Fetch,
    pub merge: MergeRule,
}

impl Augmentation {
    /// Resolve → fetch → merge the matching detail record onto `row` in place.
    /// The per-row unit the two-pass detail pass drives.
    pub async fn augment_row(
        &self,
        master_id_column: &str,
        row: &mut Record<CborValue>,
        catalog: &VistaCatalog,
    ) -> Result<()> {
        if let Some(detail) = self.fetch_one(master_id_column, row, catalog).await? {
            self.merge.apply(row, &detail);
        }
        Ok(())
    }

    /// Fetch the single detail record for one master row, or `None` if there is
    /// no match.
    ///
    /// `Id` and id-keyed `Column` sources read by key via
    /// [`get_value`](vantage_dataset::traits::ReadableValueSet::get_value) — the
    /// uniform "one record by key" primitive (cmd runs its detail script, SQL a
    /// `WHERE id =`, REST a `GET /{id}`). Other-column and `Build` sources narrow
    /// the detail vista and take the first record.
    async fn fetch_one(
        &self,
        master_id_column: &str,
        row: &Record<CborValue>,
        catalog: &VistaCatalog,
    ) -> Result<Option<Record<CborValue>>> {
        let base = catalog.build_vista(&self.table)?;
        match &self.fetch {
            Fetch::PerRow => match &self.source {
                // `get_value_with_row` hands the cheap master row to drivers that
                // use it (a cmd detail script reads list-pass columns); other
                // drivers fall through to `get_value` by default.
                Source::Id => {
                    base.get_value_with_row(&self.key(row, master_id_column)?, row)
                        .await
                }
                Source::Column { from, to: None } => {
                    base.get_value_with_row(&self.key(row, from)?, row).await
                }
                Source::Column {
                    from,
                    to: Some(col),
                } => {
                    let mut base = base;
                    self.narrow_eq(&mut base, col, from, row)?;
                    Ok(base.get_some_value().await?.map(|(_, r)| r))
                }
                Source::Build(f) => Ok(f(row, base)?.get_some_value().await?.map(|(_, r)| r)),
            },
            Fetch::Custom(f) => {
                let detail = self.resolve_detail(master_id_column, row, catalog)?;
                Ok(f(detail).await?.into_iter().next())
            }
            Fetch::Batched { .. } => Err(error!(
                "augment: batched fetch is not yet implemented (phase 2)"
            )),
        }
    }

    /// Build the detail vista and narrow it per [`Source`] — the form a
    /// [`Fetch::Custom`] closure receives.
    fn resolve_detail(
        &self,
        master_id_column: &str,
        row: &Record<CborValue>,
        catalog: &VistaCatalog,
    ) -> Result<Vista> {
        let mut base = catalog.build_vista(&self.table)?;
        match &self.source {
            Source::Id => {
                let detail_id = self.detail_id_column(&base)?;
                self.narrow_eq(&mut base, &detail_id, master_id_column, row)?;
                Ok(base)
            }
            Source::Column { from, to } => {
                let fk = match to {
                    Some(c) => c.clone(),
                    None => self.detail_id_column(&base)?,
                };
                self.narrow_eq(&mut base, &fk, from, row)?;
                Ok(base)
            }
            Source::Build(f) => f(row, base),
        }
    }

    fn narrow_eq(
        &self,
        base: &mut Vista,
        detail_column: &str,
        master_field: &str,
        row: &Record<CborValue>,
    ) -> Result<()> {
        Relation::single_key(
            "augment",
            &self.table,
            ReferenceKind::HasOne,
            detail_column.to_string(),
            master_field.to_string(),
        )
        .narrow(base, row)
    }

    fn detail_id_column(&self, base: &Vista) -> Result<String> {
        base.get_id_column().map(str::to_string).ok_or_else(|| {
            error!(
                "augment: detail vista has no id column",
                table = self.table.as_str()
            )
        })
    }

    /// Read a master row field as a scalar key string.
    fn key(&self, row: &Record<CborValue>, field: &str) -> Result<String> {
        match row.get(field) {
            Some(CborValue::Text(s)) => Ok(s.clone()),
            Some(CborValue::Integer(i)) => Ok(i128::from(*i).to_string()),
            Some(_) => Err(error!(
                "augment: key field is not a string/int",
                field = field
            )),
            None => Err(error!(
                "augment: master row missing key field",
                field = field
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_catalog() -> Arc<VistaCatalog> {
        Arc::new(VistaCatalog::new())
    }

    fn spec(source: SourceSpec, fetch: FetchSpec) -> AugmentSpec {
        AugmentSpec {
            table: "detail".into(),
            source,
            fetch,
            merge: vec![],
        }
    }

    #[test]
    fn lowers_id_source_with_default_per_row_fetch() {
        let aug =
            lower_augment(spec(SourceSpec::Id, FetchSpec::default()), &empty_catalog()).unwrap();
        assert!(matches!(aug.source, Source::Id));
        assert!(matches!(aug.fetch, Fetch::PerRow));
    }

    #[test]
    fn lowers_column_source() {
        let s = SourceSpec::Column {
            from: "key".into(),
            to: None,
        };
        let aug = lower_augment(spec(s, FetchSpec::default()), &empty_catalog()).unwrap();
        match aug.source {
            Source::Column { from, to } => {
                assert_eq!(from, "key");
                assert!(to.is_none());
            }
            _ => panic!("expected Column source"),
        }
    }

    #[test]
    fn scripted_fetch_is_rejected_for_now() {
        let s = spec(SourceSpec::Id, FetchSpec::Script { code: "x".into() });
        assert!(lower_augment(s, &empty_catalog()).is_err());
    }

    #[cfg(not(feature = "rhai"))]
    #[test]
    fn scripted_source_errors_without_rhai() {
        let s = spec(
            SourceSpec::Script {
                code: "self".into(),
            },
            FetchSpec::default(),
        );
        assert!(lower_augment(s, &empty_catalog()).is_err());
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn scripted_source_lowers_to_build_with_rhai() {
        let s = spec(
            SourceSpec::Script {
                code: "self".into(),
            },
            FetchSpec::default(),
        );
        let aug = lower_augment(s, &empty_catalog()).unwrap();
        assert!(matches!(aug.source, Source::Build(_)));
    }

    #[test]
    fn merge_overwrites_master_columns_on_clash() {
        let rule = MergeRule { columns: vec![] };
        let mut dest: Record<CborValue> = [("id".to_string(), CborValue::Text("master".into()))]
            .into_iter()
            .collect();
        let detail: Record<CborValue> = [
            ("id".to_string(), CborValue::Text("detail".into())),
            ("extra".to_string(), CborValue::Text("v".into())),
        ]
        .into_iter()
        .collect();

        rule.apply(&mut dest, &detail);

        // Detail wins on a clash (it's the authoritative hydration); new columns add.
        assert_eq!(dest.get("id"), Some(&CborValue::Text("detail".into())));
        assert_eq!(dest.get("extra"), Some(&CborValue::Text("v".into())));
    }

    #[test]
    fn merge_respects_explicit_column_list() {
        let rule = MergeRule {
            columns: vec!["extra".into()],
        };
        let mut dest: Record<CborValue> = Record::default();
        let detail: Record<CborValue> = [
            ("extra".to_string(), CborValue::Text("v".into())),
            ("skipme".to_string(), CborValue::Text("no".into())),
        ]
        .into_iter()
        .collect();

        rule.apply(&mut dest, &detail);

        assert_eq!(dest.get("extra"), Some(&CborValue::Text("v".into())));
        assert!(dest.get("skipme").is_none());
    }
}
