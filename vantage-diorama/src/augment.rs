//! Generic augmentation: enrich a master Vista's rows from a *second* Vista,
//! loaded one row at a time and merged on top.
//!
//! The master is listed; for each visible row an [`Augmentation`] finds its
//! detail Vista ([`Detail`]), narrows it for that row, fetches a record, and
//! merges chosen columns onto the master row. The detail source may be the
//! same Vista as the master (cmd's two-pass) or an entirely different backend
//! (REST master enriched by a cmd script, or vice versa).
//!
//! Runtime, closure-based form: consumers build an [`Augmentation`] directly —
//! `Source::Build` and `Fetch::Custom` take plain Rust closures.
//!
//! There is deliberately no serde/YAML mirror of these types: every consumer
//! describes its augmentation in its own config shape and constructs
//! [`Augmentation`] in Rust.

mod fetch;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Vista, VistaResolver};

/// Narrow a freshly resolved `base` detail Vista for one master `row`. Written
/// by hand; a consumer wanting a scripted narrowing builds the closure itself.
pub type BuildFn = Arc<dyn Fn(&Record<CborValue>, Vista) -> Result<Vista> + Send + Sync>;

/// Pull records from a narrowed detail Vista. The second argument is the
/// master row being augmented, as listed: the hint a detail source that
/// can't be narrowed far enough (a cmd row script) reads its inputs from.
pub type FetchFn = Arc<
    dyn Fn(
            Vista,
            Record<CborValue>,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<Record<CborValue>>>> + Send>>
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
    /// Caller-supplied fetch, handed the narrowed detail Vista and the row.
    Custom(FetchFn),
}

/// Where an augmentation's detail records come from.
pub enum Detail {
    /// Resolve the base detail Vista by `name` through `resolver`, per fetch:
    /// the form a config can describe, since a name is all it carries.
    Resolved {
        resolver: Arc<dyn VistaResolver>,
        name: String,
    },
    /// A fixed secondary Vista handle — for get-only side tables that no
    /// resolver knows (a folder-size vista keyed by path). Each fetch narrows
    /// its own clone of it.
    Fixed(Arc<Vista>),
}

impl Detail {
    /// The detail model's name, for relation labels and error context.
    fn name(&self) -> &str {
        match self {
            Detail::Resolved { name, .. } => name,
            Detail::Fixed(vista) => vista.name(),
        }
    }

    /// An owned base detail Vista for one fetch: a resolved detail is looked
    /// up by name, a fixed one clones the shared handle.
    fn base_vista(&self) -> Result<Vista> {
        match self {
            Detail::Resolved { resolver, name } => resolver.vista(name),
            Detail::Fixed(vista) => Ok(Vista::clone(vista)),
        }
    }
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
    /// The detail model this augmentation reads from.
    pub detail: Detail,
    pub source: Source,
    pub fetch: Fetch,
    pub merge: MergeRule,
}

impl Augmentation {
    /// Fetch → merge the matching detail record onto `row` in place: the
    /// per-row unit the two-pass detail pass drives. `dio_name` identifies the
    /// owning Dio in log lines — two completion series on one key with
    /// different `dio=` values means two Dios augment the same path.
    pub async fn augment_row(
        &self,
        dio_name: &str,
        master_id_column: &str,
        row: &mut Record<CborValue>,
    ) -> Result<()> {
        match self.fetch_one(master_id_column, row).await? {
            Some(detail) => {
                self.merge.apply(row, &detail);
                let merged: Vec<String> = detail
                    .iter()
                    .filter(|(k, _)| self.merge.wants(k))
                    .map(|(k, v)| format!("{k}={}", scalar_text(v)))
                    .collect();
                tracing::info!(
                    target: "vantage_diorama::augment",
                    dio = %dio_name,
                    detail = %self.detail.name(),
                    key = %self.key_display(row, master_id_column),
                    merged = %merged.join(" "),
                    "augment completed",
                );
            }
            None => {
                tracing::debug!(
                    target: "vantage_diorama::augment",
                    dio = %dio_name,
                    detail = %self.detail.name(),
                    key = %self.key_display(row, master_id_column),
                    "augment found no detail record — row stays as listed",
                );
            }
        }
        Ok(())
    }

    /// The row's augment key value, for log lines — the `Column` source's
    /// field (e.g. the folder path), else the master id.
    fn key_display(&self, row: &Record<CborValue>, master_id_column: &str) -> String {
        let field = match &self.source {
            Source::Column { from, .. } => from.as_str(),
            Source::Id | Source::Build(_) => master_id_column,
        };
        row.get(field).map(scalar_text).unwrap_or_default()
    }
}

/// A cell's short text form for log lines: scalars print directly, anything
/// nested prints a marker (log lines must stay cheap).
fn scalar_text(v: &CborValue) -> String {
    match v {
        CborValue::Text(s) => s.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        _ => "<nested>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
