//! Statement composition and the read path: every statement starts from
//! [`SpacetimeTableShell::select`] or [`SpacetimeTableShell::narrowing`], so
//! the shell's conditions bind reads, writes and subscriptions alike.

use ciborium::Value as CborValue;
use spacetimedb_sats::{AlgebraicType, AlgebraicValue, ProductValue};
use vantage_core::{Result, error};

use super::{Rec, SpacetimeTableShell};
use crate::sql::{Cond, Ident, Literal, Select};
use crate::value::{decode_sql_response, row_to_record};

impl SpacetimeTableShell {
    /// This relation, as a validated identifier.
    pub(super) fn relation_ident(&self) -> Result<Ident> {
        Ident::new(&self.relation)
    }

    /// The declared type of a column, from the module schema, so a literal is
    /// rendered for the column it addresses rather than for whatever the
    /// string happens to look like.
    pub(super) fn column_type(&self, column: &str) -> Option<&AlgebraicType> {
        self.row_type
            .elements
            .iter()
            .find(|e| e.name().is_some_and(|name| &**name == column))
            .map(|e| &e.algebraic_type)
    }

    /// The shell's conditions, for a statement's `WHERE`.
    pub(super) fn narrowing(&self) -> Vec<Cond> {
        self.conditions.clone()
    }

    pub(super) fn select(&self) -> Result<Select> {
        Ok(Select::new(self.relation_ident()?).where_all(self.narrowing()))
    }

    /// `<key> = <id>`, with the id rendered for the key column's declared type.
    pub(super) fn id_is(&self, column: &str, id: &str) -> Result<Cond> {
        Ok(Cond::eq(
            Ident::new(column)?,
            Literal::for_id(self.column_type(column), id)?,
        ))
    }

    /// Run a query and pair every row with its Vista id.
    pub(super) async fn fetch(&self, sql: &str) -> Result<Vec<(String, Rec)>> {
        let body = self.db.sql(sql).await?;
        let decoded = decode_sql_response(&body)?;
        let mut out = Vec::with_capacity(decoded.rows.len());
        for row in &decoded.rows {
            // The response's own schema, not the table's: a projection returns
            // fewer columns than the row type declares.
            let record = row_to_record(&decoded.columns, &decoded.schema, row);
            out.push((self.row_id(&record, row)?, record));
        }
        Ok(out)
    }

    /// Row `id`, when it is in the set.
    pub(super) async fn get_row(&self, id: &str) -> Result<Option<Rec>> {
        match self.identity.column() {
            // Keyed: the id goes into the server's `WHERE` next to the set's
            // conditions, so a row outside the set is not reachable just
            // because its id is known.
            Some(column) => {
                let sql = self.select()?.and_where(self.id_is(column, id)?).render();
                Ok(self.fetch(&sql).await?.into_iter().next().map(|(_, r)| r))
            }
            // Keyless: the id is a hash of the row, so there is no `WHERE` to
            // write. Reading the set and matching is the only way to address a
            // row in a table with no key.
            None => Ok(self
                .fetch(&self.select()?.render())
                .await?
                .into_iter()
                .find(|(row_id, _)| row_id == id)
                .map(|(_, record)| record)),
        }
    }

    pub(super) async fn count_rows(&self) -> Result<i64> {
        let sql = self.select()?.count_as("n").render();
        let body = self.db.sql(&sql).await?;
        let decoded = decode_sql_response(&body)?;
        let Some(row) = decoded.rows.first() else {
            return Ok(0);
        };
        match row.elements.first() {
            Some(AlgebraicValue::U64(n)) => Ok(*n as i64),
            Some(AlgebraicValue::I64(n)) => Ok(*n),
            other => Err(error!(
                "COUNT(*) did not return an integer",
                got = format!("{other:?}")
            )),
        }
    }

    /// The Vista id of a row.
    fn row_id(&self, record: &Rec, raw: &ProductValue) -> Result<String> {
        match self.identity.column() {
            Some(column) => {
                let value = record.get(column).ok_or_else(|| {
                    error!(
                        "row is missing its identity column — was the projection narrowed?",
                        table = self.relation.as_str(),
                        column = column
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

/// Stringify a CBOR value for use as a Vista id.
pub(super) fn cbor_to_id(value: &CborValue) -> String {
    match value {
        // As in `Literal::from_cbor`: the tag annotates, the payload is the id.
        // Left in, it would fall to the `Debug` arm below and produce an id no
        // `WHERE` could ever match.
        CborValue::Tag(_, inner) => cbor_to_id(inner),
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
pub(super) fn content_hash(row: &ProductValue) -> String {
    let bytes = spacetimedb_sats::bsatn::to_vec(row).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
