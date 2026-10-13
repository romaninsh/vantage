//! `SpacetimeTableShell` — one SpacetimeDB table or view, behind the `TableShell`
//! boundary.
//!
//! # This driver implements only what the server does
//!
//! SpacetimeDB's SQL has no `ORDER BY`, `OFFSET`, `GROUP BY`, `IN` or `LIKE`. It
//! has `WHERE` with the six comparison operators, `LIMIT`, and `COUNT(*)`.
//!
//! So this shell implements exactly what the server can answer and leaves the
//! rest on the trait defaults, which refuse with `Unsupported`. **Nothing is
//! emulated client-side.** There is no local sort, no substring scan, no
//! fetch-everything-and-slice.
//!
//! Emulation would make a capability describe a fiction in the other direction:
//! the set says "no order" while ordering appears to work, so a consumer cannot
//! tell a cheap server-side sort from a full-table download. A caller that
//! checks the capabilities first, as the contract asks, never sees a refusal.
//!
//! The practical consequence: a grid over a SpacetimeDB table shows rows in the
//! server's natural order, and its sort headers are inert.
//!
//! A clone shares the connection and copies the condition list, so narrowing a
//! clone leaves the original alone.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use spacetimedb_sats::ProductType;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{
    Capability, Condition, Page, Preview, TableShell, VistaCapabilities, VistaChangeStream,
    VistaMetadata, unsupported,
};

use crate::client::SpacetimeDb;
use crate::schema::RowIdentity;
use crate::sql::{Cond, Ident};

mod reads;
#[cfg(test)]
mod tests;
mod watch;
mod writes;

type Rec = Record<CborValue>;

#[derive(Clone)]
pub struct SpacetimeTableShell {
    db: SpacetimeDb,
    /// Table or view name, as it appears in `FROM`.
    relation: String,
    metadata: VistaMetadata,
    identity: RowIdentity,
    /// The row's SATS type, from the module schema.
    ///
    /// Needed by the change feed, whose pushed rows arrive as raw BSATN with no
    /// self-description, and by id rendering, which types a literal for the
    /// column it addresses. (The read path gets its schema back with each SQL
    /// response.)
    row_type: ProductType,
    /// The narrowing, already rendered for `WHERE`. Every read, write and
    /// subscription is composed from it.
    conditions: Vec<Cond>,
    capabilities: VistaCapabilities,
}

impl SpacetimeTableShell {
    pub(crate) fn new(
        db: SpacetimeDb,
        relation: String,
        metadata: VistaMetadata,
        identity: RowIdentity,
        row_type: ProductType,
        capabilities: VistaCapabilities,
    ) -> Self {
        Self {
            db,
            relation,
            metadata,
            identity,
            row_type,
            conditions: Vec::new(),
            capabilities,
        }
    }
}

#[async_trait]
impl TableShell for SpacetimeTableShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "spacetimedb"
    }

    /// The `SELECT` a list sends. A subscription sends the same statement.
    fn preview(&self) -> Preview {
        let preview = Preview::new();
        match self.select() {
            Ok(select) => preview.with_query(select.render()),
            Err(_) => preview.with_note("the relation name cannot be quoted as an identifier"),
        }
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        Ok(self
            .fetch(&self.select()?.render())
            .await?
            .into_iter()
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        self.get_row(id).await
    }

    async fn count(&self) -> Result<i64> {
        self.count_rows().await
    }

    /// The first `limit` rows, through the dialect's `LIMIT`. There is no
    /// `OFFSET`, so a window that starts further in is refused, and `Window`
    /// is not advertised.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        if offset > 0 {
            return unsupported(Capability::Window);
        }
        let sql = self.select()?.limit(limit as u64).render();
        Ok(Page::new(self.fetch(&sql).await?))
    }

    /// See the `watch` module.
    async fn watch(&self) -> Result<VistaChangeStream> {
        watch::watch(self).await
    }

    /// A comparison on a declared column, pushed into `WHERE`. The dialect
    /// spells `=`, `!=`, `>`, `>=`, `<` and `<=`; `InSet`, `NotInSet` and
    /// `Like` have no spelling and are refused as `Unsupported`, so the caller
    /// evaluates them over the rows it reads. A value with no SQL literal (a
    /// null, an array) is refused as `Unsupported` for the same reason.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let column = condition.column.as_str();
        if !self.metadata.columns.contains_key(column) {
            return Err(error!(
                "Unknown column for condition",
                table = self.relation.as_str(),
                field = column
            ));
        }
        let cond = Cond::new(Ident::new(column)?, condition.op, &condition.value)?;
        self.conditions.push(cond);
        Ok(())
    }

    // Writes are SQL DML keyed on the row identity; see the `writes` module.

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        self.insert_row_with_id(id, record).await
    }

    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace_row(id, record).await
    }

    /// The same write as `replace`, which already creates a missing row.
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace_row(id, record).await
    }

    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        self.patch_row(id, partial).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.delete_row(id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.delete_rows().await
    }

    // A SpacetimeDB module declares no foreign keys, so the shell has no
    // references and `get_ref` keeps the trait's `Traverse` refusal.
}
