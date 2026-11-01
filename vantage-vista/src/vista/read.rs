//! Reads beyond the `ReadableValueSet` surface, and reference traversal.
//! Every read fills the Vista's computed columns.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use super::Vista;
use crate::{
    capabilities::Capability,
    change::{VistaChange, VistaChangeStream},
    computed,
    denials::Denials,
    page::Page,
};

impl Vista {
    pub async fn count(&self) -> Result<i64> {
        self.shell().count().await
    }

    /// Rows `[offset, offset + limit)` of the set, in its order; see
    /// [`TableShell::fetch_window`](crate::TableShell::fetch_window). The
    /// leading window (`offset == 0`) is always asked of the shell; any
    /// other offset needs [`Window`](Capability::Window).
    pub async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        if offset > 0 {
            self.require(Capability::Window)?;
        }
        let mut page = self.shell().fetch_window(offset, limit).await?;
        self.fill_computed_rows(page.rows.iter_mut().map(|(_, r)| r))?;
        Ok(page)
    }

    /// Up to `limit` rows after `token`, and the next token; see
    /// [`TableShell::fetch_cursor`](crate::TableShell::fetch_cursor). Needs
    /// [`Cursor`](Capability::Cursor).
    pub async fn fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        self.require(Capability::Cursor)?;
        let (mut page, next) = self.shell().fetch_cursor(token, limit).await?;
        self.fill_computed_rows(page.rows.iter_mut().map(|(_, r)| r))?;
        Ok((page, next))
    }

    /// Subscribe to changes; see [`TableShell::watch`](crate::TableShell::watch).
    /// Needs [`Subscribe`](Capability::Subscribe). Row-bearing changes carry
    /// the computed columns too.
    pub async fn watch(&self) -> Result<VistaChangeStream> {
        self.require(Capability::Subscribe)?;
        let mut changes = self.shell().watch().await?;
        let columns = self.computed_columns();
        if columns.is_empty() {
            return Ok(changes);
        }
        Ok(Box::pin(async_stream::stream! {
            while let Some(change) =
                std::future::poll_fn(|cx| changes.as_mut().poll_next(cx)).await
            {
                yield change.and_then(|change| match change {
                    VistaChange::Inserted { id, mut value } => {
                        computed::fill(&columns, &mut value)?;
                        Ok(VistaChange::Inserted { id, value })
                    }
                    VistaChange::Updated { id, mut value } => {
                        computed::fill(&columns, &mut value)?;
                        Ok(VistaChange::Updated { id, value })
                    }
                    other => Ok(other),
                });
            }
        }))
    }

    /// The target of `relation`: with a parent `row` from this Vista, the
    /// related rows; with `None`, the bare target a new related row goes
    /// into. Contained relations come back as a Vista over the embedded
    /// records.
    ///
    /// A has-one key may be a computed column of this Vista (the row carries
    /// its value). A has-many key that is a computed column of the target
    /// can't narrow it: a shell narrows through
    /// [`Reference::narrow`](crate::Reference::narrow), whose
    /// [`Vista::add_condition`] on the target refuses a computed column, so
    /// the traversal fails as `Unsupported` naming the relation and column.
    /// The bare target (`None`) is only described, so it is not refused.
    ///
    /// A contained target writes through the parent row, so under a parent
    /// that takes no updates, or whose host column a rule made read-only, it
    /// takes no writes either: they are refused `Forbidden`, naming the
    /// parent's rule.
    pub fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let target = self.shell().get_ref(relation, row)?;
        let Some(reference) = self.reference(relation).filter(|r| r.kind.is_contained()) else {
            return Ok(target);
        };
        let host_rule = reference
            .host_column
            .as_deref()
            .and_then(|host| self.denied_column(host));
        let rule = match (self.denied(Capability::Update), host_rule) {
            (Some(rule), _) | (None, Some(rule)) => rule.to_string(),
            (None, None) if self.capabilities().has(Capability::Update) => return Ok(target),
            (None, None) => format!("parent {} takes no updates", self.name()),
        };
        let mut denials = Denials::new();
        for write in [
            Capability::Insert,
            Capability::Update,
            Capability::Delete,
            Capability::DeleteAll,
        ] {
            denials.deny(write, rule.as_str());
        }
        Ok(target.deny(denials))
    }

    /// Lock the rows the next reads return until the transaction this Vista
    /// is wrapped in ends; see [`TableShell::set_lock`](crate::TableShell::set_lock).
    pub fn lock(&mut self) -> Result<()> {
        self.shell_mut().set_lock()
    }

    /// Driver-native bulk load, returning how many rows were newly inserted;
    /// see [`TableShell::import`](crate::TableShell::import).
    pub async fn import_values(
        &self,
        rows: &indexmap::IndexMap<String, Record<CborValue>>,
    ) -> Result<usize> {
        self.require_write(Capability::Import, "import")?;
        let rows = self.without_computed_all(rows);
        self.shell().import(&rows).await
    }
}
