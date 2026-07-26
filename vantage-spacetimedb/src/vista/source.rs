//! `SpacetimeTableShell` — one SpacetimeDB table or view, behind the `TableShell`
//! boundary.
//!
//! # This driver implements only what the server does
//!
//! SpacetimeDB's SQL has no `ORDER BY`, `OFFSET`, `GROUP BY`, `IN` or `LIKE`. It
//! has `WHERE` with the six comparison operators, `LIMIT`, and `COUNT(*)`.
//!
//! So this shell overrides exactly those methods the server can answer, and
//! leaves every other `TableShell` method on its trait default — which reports
//! `Unsupported` against a `false` capability flag. **Nothing is emulated
//! client-side.** There is no local sort, no substring scan, no
//! fetch-everything-and-slice.
//!
//! That is a deliberate departure from the drivers that do emulate (see
//! `vantage-kubernetes`, which materialises the whole collection and sorts it in
//! memory). Emulation makes a capability flag a lie in the other direction: the
//! flag says `can_order: false` while ordering appears to work, so a consumer
//! cannot tell a cheap server-side sort from a full-table download. Refusing is
//! honest, it is what `VistaCapabilities` documents, and a caller that checks the
//! flag first — as the contract asks — never sees the refusal at all.
//!
//! The practical consequence: a grid over a SpacetimeDB table shows rows in the
//! server's natural order, and its sort headers are inert. That is the truth
//! about this backend.

use std::collections::HashSet;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use spacetimedb_sats::ProductType;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{
    Column as VistaColumn, FilterOp, Reference as VistaReference, TableShell, Vista,
    VistaCapabilities, VistaChange, VistaChangeStream, VistaMetadata,
};

use crate::client::SpacetimeDb;
use crate::schema::RowIdentity;
use crate::value::{decode_sql_response, row_to_record};

/// A condition pushed down into the query's `WHERE` clause.
#[derive(Debug, Clone)]
struct Condition {
    field: String,
    op: FilterOp,
    value: CborValue,
}

pub struct SpacetimeTableShell {
    db: SpacetimeDb,
    /// Table or view name, as it appears in `FROM`.
    relation: String,
    metadata: VistaMetadata,
    identity: RowIdentity,
    /// The row's SATS type, from the module schema.
    ///
    /// Needed only by the change feed: pushed rows arrive as raw BSATN with no
    /// self-description, so they can only be decoded against the type the schema
    /// declares. (The read path gets its schema back with each SQL response.)
    row_type: ProductType,
    conditions: Vec<Condition>,
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

    /// Column names in the row type's own order — the order a pushed BSATN row
    /// decodes into.
    fn columns_in_order(&self) -> Vec<String> {
        self.row_type
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                e.name()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| format!("column_{i}"))
            })
            .collect()
    }

    /// `WHERE …` for the current conditions, or an empty string.
    fn where_clause(&self) -> Result<String> {
        if self.conditions.is_empty() {
            return Ok(String::new());
        }
        let mut parts = Vec::with_capacity(self.conditions.len());
        for condition in &self.conditions {
            parts.push(format!(
                "{} {} {}",
                quote_ident(&condition.field),
                sql_operator(condition.op)?,
                sql_literal(&condition.value)?
            ));
        }
        Ok(format!(" WHERE {}", parts.join(" AND ")))
    }

    fn select_sql(&self) -> Result<String> {
        Ok(format!(
            "SELECT * FROM {}{}",
            quote_ident(&self.relation),
            self.where_clause()?
        ))
    }

    /// Run a query and pair every row with its Vista id.
    async fn fetch(&self, sql: &str) -> Result<Vec<(String, Record<CborValue>)>> {
        let body = self.db.sql(sql).await?;
        let decoded = decode_sql_response(&body)?;
        let mut out = Vec::with_capacity(decoded.rows.len());
        for row in &decoded.rows {
            let record = row_to_record(&decoded.columns, row);
            out.push((self.row_id(&record, row)?, record));
        }
        Ok(out)
    }

    /// Refuse a write the connection or the relation cannot perform, before any
    /// request is sent.
    ///
    /// Worth catching here rather than letting the host answer: an anonymous
    /// caller gets a permission error that says nothing about *why*, and a write
    /// to a view fails in a way that reads like a server fault rather than a
    /// category error.
    fn require_writable(&self, what: &str) -> Result<()> {
        if self.capabilities.can_insert
            || self.capabilities.can_update
            || self.capabilities.can_delete
        {
            return Ok(());
        }
        let reason = if !self.db.has_token() {
            "no token is configured on the datasource, and SpacetimeDB refuses SQL DML \
             from anonymous callers"
        } else {
            "SQL DML is restricted to the database owner, and this connection's identity \
             is not it — use a reducer for writes by other callers"
        };
        Err(error!(
            format!("cannot {what} through this relation: {reason}"),
            table = self.relation.clone(),
            operation = what.to_string()
        )
        .mark_unsupported())
    }

    /// `INSERT INTO t (cols…) VALUES (…)` for one row.
    async fn insert_row(&self, row: &Record<CborValue>) -> Result<()> {
        let mut columns = Vec::with_capacity(row.len());
        let mut values = Vec::with_capacity(row.len());
        for (name, value) in row.iter() {
            // A column the module does not declare would be rejected by the host
            // with a parse error; dropping it here keeps a Vista's extra
            // bookkeeping fields from breaking an otherwise valid insert.
            if !self.metadata.columns.contains_key(name) {
                continue;
            }
            columns.push(quote_ident(name));
            values.push(sql_literal(value)?);
        }
        if columns.is_empty() {
            return Err(error!(
                "nothing to insert — no field matched a column of this table",
                table = self.relation.clone()
            ));
        }
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quote_ident(&self.relation),
            columns.join(", "),
            values.join(", ")
        );
        self.db.sql(&sql).await.map(|_| ())
    }

    /// Derive the Vista id for a row.
    fn row_id(
        &self,
        record: &Record<CborValue>,
        raw: &spacetimedb_sats::ProductValue,
    ) -> Result<String> {
        match self.identity.column() {
            Some(column) => {
                let value = record.get(column).ok_or_else(|| {
                    error!(
                        "row is missing its identity column — was the projection narrowed?",
                        table = self.relation.clone(),
                        column = column.to_string()
                    )
                })?;
                Ok(cbor_to_id(value))
            }
            // No key of any kind: hash the row's own bytes. Sound here because a
            // SpacetimeDB delete carries the whole row, so a delete hashes to the
            // same id as the insert that created it.
            None => Ok(content_hash(raw)),
        }
    }
}

#[async_trait]
impl TableShell for SpacetimeTableShell {
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(self.fetch(&self.select_sql()?).await?.into_iter().collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        match self.identity.column() {
            // Keyed: narrow with the server's own `WHERE`.
            Some(column) => {
                let sql = format!(
                    "SELECT * FROM {} WHERE {} = {}{}",
                    quote_ident(&self.relation),
                    quote_ident(column),
                    id_literal(id),
                    // Existing conditions still apply: a row outside the vista's
                    // set must not be reachable just because its id is known.
                    self.where_clause()?.replacen(" WHERE ", " AND ", 1)
                );
                Ok(self.fetch(&sql).await?.into_iter().next().map(|(_, r)| r))
            }
            // Keyless: the id *is* a hash of the row, so there is no `WHERE` to
            // write. This reads the set and matches — not an emulated feature,
            // but the only way to address a row in a table with no key.
            None => Ok(self
                .fetch(&self.select_sql()?)
                .await?
                .into_iter()
                .find(|(row_id, _)| row_id == id)
                .map(|(_, record)| record)),
        }
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        // `LIMIT` is one of the few things the dialect does have.
        let sql = format!("{} LIMIT 1", self.select_sql()?);
        Ok(self.fetch(&sql).await?.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        let sql = format!(
            "SELECT COUNT(*) AS n FROM {}{}",
            quote_ident(&self.relation),
            self.where_clause()?
        );
        let body = self.db.sql(&sql).await?;
        let decoded = decode_sql_response(&body)?;
        let Some(row) = decoded.rows.first() else {
            return Ok(0);
        };
        match row.elements.first() {
            Some(spacetimedb_sats::AlgebraicValue::U64(n)) => Ok(*n as i64),
            Some(spacetimedb_sats::AlgebraicValue::I64(n)) => Ok(*n),
            other => Err(error!(
                "COUNT(*) did not return an integer",
                got = format!("{other:?}")
            )),
        }
    }

    // ---- Writes ------------------------------------------------------------
    //
    // SQL DML, keyed on the row identity. Reducers are the write path a module
    // actually sanctions — they enforce its own rules, which DML bypasses — and
    // are reached through `SpacetimeDb::call_reducer` and surfaced as actions
    // rather than through the value-set surface.

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.require_writable("insert")?;
        let id_column = self.identity.column().ok_or_else(|| {
            error!(
                "cannot insert by id into a table with no key — its id is a hash of \
                 the row, which does not exist until the row does",
                table = self.relation.clone()
            )
            .mark_unsupported()
        })?;

        // The caller's id wins over any value in the record, so the row lands
        // where the caller said it would.
        let mut row = record.clone();
        row.insert(id_column.to_string(), CborValue::Text(id.clone()));

        self.insert_row(&row).await?;
        self.get_vista_value(_vista, id).await?.ok_or_else(|| {
            error!(
                "inserted row could not be read back",
                table = self.relation.clone(),
                id = id.clone()
            )
        })
    }

    async fn replace_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        // A replace is a patch of every column: SpacetimeDB has no `UPSERT`, and
        // delete-then-insert would briefly remove the row from every live
        // subscription watching it.
        self.patch_vista_value(vista, id, record).await
    }

    async fn patch_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.require_writable("update")?;
        let id_column = self.identity.column().ok_or_else(|| {
            error!(
                "cannot update a row in a table with no key: there is no `WHERE` that \
                 identifies it",
                table = self.relation.clone()
            )
            .mark_unsupported()
        })?;

        let assignments = partial
            .iter()
            // Never write the key column: an `UPDATE` that moves a row's identity
            // would make the id the caller passed refer to nothing.
            .filter(|(name, _)| name.as_str() != id_column)
            .map(|(name, value)| Ok(format!("{} = {}", quote_ident(name), sql_literal(value)?)))
            .collect::<Result<Vec<_>>>()?;

        if assignments.is_empty() {
            return self.get_vista_value(vista, id).await?.ok_or_else(|| {
                error!("no such row", table = self.relation.clone(), id = id.clone())
            });
        }

        let sql = format!(
            "UPDATE {} SET {} WHERE {} = {}",
            quote_ident(&self.relation),
            assignments.join(", "),
            quote_ident(id_column),
            id_literal(id)
        );
        self.db.sql(&sql).await?;

        self.get_vista_value(vista, id).await?.ok_or_else(|| {
            error!(
                "updated row could not be read back — did the update move it out of \
                 this set?",
                table = self.relation.clone(),
                id = id.clone()
            )
        })
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.require_writable("delete")?;
        let id_column = self.identity.column().ok_or_else(|| {
            error!(
                "cannot delete a row in a table with no key: there is no `WHERE` that \
                 identifies it",
                table = self.relation.clone()
            )
            .mark_unsupported()
        })?;
        let sql = format!(
            "DELETE FROM {} WHERE {} = {}",
            quote_ident(&self.relation),
            quote_ident(id_column),
            id_literal(id)
        );
        self.db.sql(&sql).await.map(|_| ())
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.require_writable("delete")?;
        // Honours the vista's conditions: deleting "all" of a narrowed set must
        // not empty the whole table.
        let sql = format!(
            "DELETE FROM {}{}",
            quote_ident(&self.relation),
            self.where_clause()?
        );
        self.db.sql(&sql).await.map(|_| ())
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.add_op_condition(field, FilterOp::Eq, value)
    }

    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        if !self.metadata.columns.contains_key(field) {
            return Err(error!(
                "unknown column",
                table = self.relation.clone(),
                field = field.to_string()
            ));
        }
        // `InSet`/`NotInSet` have no `IN` to lower onto. Reporting Unimplemented
        // tells the consumer to filter locally itself — dropping the condition
        // silently would return too many rows, which is worse than a refusal.
        if matches!(op, FilterOp::InSet | FilterOp::NotInSet) {
            return Err(error!(
                "SpacetimeDB SQL has no `IN`, so this filter cannot be pushed down; \
                 filter locally instead",
                table = self.relation.clone(),
                field = field.to_string()
            )
            .mark_unimplemented());
        }
        self.conditions.push(Condition {
            field: field.to_string(),
            op,
            value: value.clone(),
        });
        Ok(())
    }

    /// Cheap: the connection is `Arc`-shared and only the condition list is
    /// copied, so a caller can narrow a copy without disturbing the original.
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(Self {
            db: self.db.clone(),
            relation: self.relation.clone(),
            metadata: self.metadata.clone(),
            identity: self.identity.clone(),
            row_type: self.row_type.clone(),
            conditions: self.conditions.clone(),
            capabilities: self.capabilities.clone(),
        }))
    }

    /// Subscribe to this set and stream every change.
    ///
    /// # The vista's conditions go into the subscription itself
    ///
    /// The query sent to the server is the same `SELECT * FROM t WHERE …` the
    /// read path builds, so the server filters and only matching rows are ever
    /// sent. That is stronger than it sounds: SpacetimeDB maintains the
    /// subscription incrementally, so a row that *changes out of* the filtered
    /// set arrives as a **delete**. Set membership is therefore correct by
    /// construction — this driver needs no re-read per notification (as the
    /// SurrealDB driver does) and no whole-set reload (as Postgres does).
    ///
    /// The exception is a condition the dialect cannot express — `InSet` has no
    /// `IN` — which `add_op_condition` refuses outright, so it can never end up
    /// silently missing from the subscription.
    ///
    /// # Deletes and inserts become one `Updated`
    ///
    /// The wire format has no update: a changed row is a delete plus an insert
    /// of the same key in one transaction. They are paired here by id, so a
    /// consumer sees `Updated` and a grid repaints a row rather than removing
    /// and re-adding it.
    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        let query = self.select_sql()?;
        let row_type = self.row_type.clone();
        let columns = self.columns_in_order();
        let identity = self.identity.clone();
        let relation = self.relation.clone();

        let mut subscription = self
            .db
            .subscribe(query, row_type.clone(), relation.clone())
            .await?;

        let stream = async_stream::try_stream! {
            while let Some(delta) = subscription.recv().await {
                let delta = delta?;

                if delta.ephemeral {
                    // Event-table rows exist only for the transaction that
                    // emitted them, so pairing them as inserts would put rows in
                    // a cache that the table itself will never list. Refuse
                    // loudly rather than populate a grid that disagrees with its
                    // own source.
                    Err(error!(
                        "SpacetimeDB sent event-table rows for this relation; their rows are \
                         deleted in the transaction that inserts them, so they cannot back a \
                         Vista",
                        table = relation.clone()
                    ).mark_unsupported())?;
                    continue;
                }

                let inserted = decode_all(&delta.inserts, &row_type, &columns, &identity)?;
                let deleted = decode_all(&delta.deletes, &row_type, &columns, &identity)?;

                // Pair by id within the transaction: an id on both sides means
                // the row changed rather than being replaced.
                let deleted_ids: HashSet<&String> = deleted.iter().map(|(id, _)| id).collect();
                let inserted_ids: HashSet<&String> = inserted.iter().map(|(id, _)| id).collect();

                for (id, record) in &inserted {
                    if deleted_ids.contains(id) {
                        yield VistaChange::Updated { id: id.clone(), value: record.clone() };
                    } else {
                        yield VistaChange::Inserted { id: id.clone(), value: record.clone() };
                    }
                }
                for (id, _) in &deleted {
                    // Unmatched deletes genuinely left the set — either removed
                    // outright, or changed so they no longer satisfy the
                    // subscription's WHERE. Both mean "drop this row".
                    if !inserted_ids.contains(id) {
                        yield VistaChange::Deleted { id: id.clone() };
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "spacetimedb"
    }

    // Deliberately NOT overridden, so each falls through to the trait default and
    // reports `Unsupported` against its `false` capability flag:
    //
    //   add_order / clear_orders   — no `ORDER BY` in the dialect
    //   add_search / clear_search  — no `LIKE`
    //   set_page_size              — nothing to page with
    //   fetch_page / fetch_next / fetch_window
    //                              — no `OFFSET`; `LIMIT` alone cannot skip
    //
    // Implementing any of these over the materialised set would make the
    // capability flags describe a fiction. See the module docs.
}

/// Decode a batch of pushed BSATN rows and pair each with its Vista id.
fn decode_all(
    rows: &[Vec<u8>],
    row_type: &ProductType,
    columns: &[String],
    identity: &RowIdentity,
) -> Result<Vec<(String, Record<CborValue>)>> {
    let mut out = Vec::with_capacity(rows.len());
    for bytes in rows {
        let value = crate::client::ws::decode_row(bytes, row_type)?;
        let record = row_to_record(columns, &value);
        let id = match identity.column() {
            Some(column) => record.get(column).map(cbor_to_id).ok_or_else(|| {
                error!(
                    "a pushed row is missing its identity column",
                    column = column.to_string()
                )
            })?,
            None => content_hash(&value),
        };
        out.push((id, record));
    }
    Ok(out)
}

/// Quote an identifier for the query.
///
/// Module identifiers are already constrained to `[A-Za-z0-9_]`, so this is
/// belt-and-braces rather than the primary defence — but a name is never
/// interpolated raw.
fn quote_ident(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

fn sql_operator(op: FilterOp) -> Result<&'static str> {
    Ok(match op {
        FilterOp::Eq => "=",
        FilterOp::Ne => "!=",
        FilterOp::Gt => ">",
        FilterOp::Gte => ">=",
        FilterOp::Lt => "<",
        FilterOp::Lte => "<=",
        FilterOp::InSet | FilterOp::NotInSet => {
            return Err(error!("no `IN` operator in SpacetimeDB SQL").mark_unimplemented());
        }
    })
}

/// Render a CBOR value as a SpacetimeDB SQL literal.
///
/// Strings are single-quoted with embedded quotes doubled — the only escaping
/// the dialect defines, and the reason a value never reaches the query
/// unprocessed.
fn sql_literal(value: &CborValue) -> Result<String> {
    Ok(match value {
        CborValue::Bool(b) => b.to_string(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Text(s) => format!("'{}'", s.replace('\'', "''")),
        CborValue::Null => {
            return Err(error!(
                "SpacetimeDB SQL has no null literal to compare against"
            )
            .mark_unimplemented());
        }
        other => {
            return Err(error!(
                "value cannot be expressed as a SpacetimeDB SQL literal",
                value = format!("{other:?}")
            )
            .mark_unimplemented());
        }
    })
}

/// Render a Vista id as a literal.
///
/// Ids arrive as strings even when the underlying column is numeric, so a purely
/// numeric id is emitted unquoted; anything else is quoted. Hex identities keep
/// their `0x` form, which is what the dialect expects.
fn id_literal(id: &str) -> String {
    if id.parse::<i128>().is_ok() || id.starts_with("0x") {
        id.to_string()
    } else {
        format!("'{}'", id.replace('\'', "''"))
    }
}

/// Stringify a CBOR value for use as a Vista id.
fn cbor_to_id(value: &CborValue) -> String {
    match value {
        CborValue::Text(s) => s.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Null => String::new(),
        other => format!("{other:?}"),
    }
}

/// A stable id for a row in a table with no key.
///
/// FNV-1a over the row's BSATN encoding: deterministic across processes and
/// versions (unlike `DefaultHasher`), which matters because this id has to match
/// between a row read over SQL and the same row arriving on a change feed.
fn content_hash(row: &spacetimedb_sats::ProductValue) -> String {
    let bytes = spacetimedb_sats::bsatn::to_vec(row).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_literals_escape_embedded_quotes() {
        assert_eq!(
            sql_literal(&CborValue::Text("O'Brien".into())).unwrap(),
            "'O''Brien'"
        );
    }

    #[test]
    fn numeric_and_boolean_literals_are_unquoted() {
        assert_eq!(sql_literal(&CborValue::Integer(42.into())).unwrap(), "42");
        assert_eq!(sql_literal(&CborValue::Bool(true)).unwrap(), "true");
    }

    #[test]
    fn null_is_refused_rather_than_rendered() {
        // There is no null literal in the dialect, so a comparison against one
        // cannot be expressed. Refusing beats emitting something that parses but
        // means the wrong thing.
        let err = sql_literal(&CborValue::Null).unwrap_err();
        assert!(err.to_string().contains("null"));
    }

    #[test]
    fn ids_are_quoted_unless_numeric_or_hex() {
        assert_eq!(id_literal("42"), "42");
        assert_eq!(id_literal("0xdeadbeef"), "0xdeadbeef");
        assert_eq!(id_literal("alice"), "'alice'");
        assert_eq!(id_literal("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn operators_cover_the_dialect_and_refuse_in() {
        assert_eq!(sql_operator(FilterOp::Eq).unwrap(), "=");
        assert_eq!(sql_operator(FilterOp::Ne).unwrap(), "!=");
        assert_eq!(sql_operator(FilterOp::Gte).unwrap(), ">=");
        assert!(sql_operator(FilterOp::InSet).is_err());
    }
}
