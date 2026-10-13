//! Reading one master row's detail record: narrowing the detail Vista for the
//! row, then fetching through it.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{JoinKey, Reference, Vista};

use super::{Augmentation, Fetch, Source};

impl Augmentation {
    /// Fetch the single detail record for one master row, or `None` if there is
    /// no match.
    ///
    /// `Id` and id-keyed `Column` sources read by key via
    /// [`get_value`](vantage_dataset::traits::ReadableValueSet::get_value) — the
    /// uniform "one record by key" primitive (cmd runs its detail script, SQL a
    /// `WHERE id =`, REST a `GET /{id}`). Other-column and `Build` sources narrow
    /// the detail vista and take the first record. A `Custom` fetch receives the
    /// narrowed detail vista and its own copy of the row.
    pub(super) async fn fetch_one(
        &self,
        master_id_column: &str,
        row: &Record<CborValue>,
    ) -> Result<Option<Record<CborValue>>> {
        let base = self.detail.base_vista()?;
        match &self.fetch {
            Fetch::PerRow => match &self.source {
                Source::Id => base.get_value(self.key(row, master_id_column)?).await,
                Source::Column { from, to: None } => base.get_value(self.key(row, from)?).await,
                Source::Column {
                    from,
                    to: Some(col),
                } => {
                    let narrowed = self.narrow_eq(base, JoinKey::new(from, col), row)?;
                    Ok(narrowed.get_some_value().await?.map(|(_, r)| r))
                }
                Source::Build(f) => Ok(f(row, base)?.get_some_value().await?.map(|(_, r)| r)),
            },
            Fetch::Custom(f) => {
                let detail = self.resolve_detail(master_id_column, row, base)?;
                Ok(f(detail, row.clone()).await?.into_iter().next())
            }
        }
    }

    /// Narrow the base detail vista per [`Source`] — the form a
    /// [`Fetch::Custom`] closure receives.
    fn resolve_detail(
        &self,
        master_id_column: &str,
        row: &Record<CborValue>,
        base: Vista,
    ) -> Result<Vista> {
        match &self.source {
            Source::Id => self.narrow_eq(base, JoinKey::to_target_id(master_id_column), row),
            Source::Column { from, to: Some(to) } => {
                self.narrow_eq(base, JoinKey::new(from, to), row)
            }
            Source::Column { from, to: None } => {
                self.narrow_eq(base, JoinKey::to_target_id(from), row)
            }
            Source::Build(f) => f(row, base),
        }
    }

    /// `base` narrowed to the rows whose `key.to` column (the detail id when
    /// unset) equals the master row's `key.from` value.
    fn narrow_eq(&self, base: Vista, key: JoinKey, row: &Record<CborValue>) -> Result<Vista> {
        Reference::has_one("augment", self.detail.name(), key).narrow(base, row)
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
