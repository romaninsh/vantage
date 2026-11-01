//! The write path: SQL DML keyed on the row identity.
//!
//! Reducers are the write path a module actually sanctions — they enforce its
//! own rules, which DML bypasses — and are reached through
//! `SpacetimeDb::call_reducer` rather than through the value-set surface.
//!
//! Every statement carries the shell's conditions next to the id, so a
//! narrowed shell can't change a row it couldn't read. The shell still doesn't
//! advertise `ConfineWrites`: an insert isn't conformed to the conditions, and
//! an update can move a row out of the set before the read-back sees it.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_vista::Capability;

use super::{Rec, SpacetimeTableShell};
use crate::sql::{Delete, Ident, Insert, Literal, Update};

impl SpacetimeTableShell {
    /// Insert row `id`. An id already in the set returns the stored row; a
    /// `None` id is refused, since the host assigns an auto-inc key and its
    /// SQL has no `RETURNING` to read it back from.
    pub(super) async fn insert_row_with_id(
        &self,
        id: Option<&str>,
        record: &Rec,
    ) -> Result<(String, Rec)> {
        self.require(Capability::Insert)?;
        let key = self.key_column()?;
        let Some(id) = id else {
            return Err(error!(
                "SpacetimeDB cannot report the id it assigned: its SQL has no `RETURNING`; \
                 supply the id, or call a reducer",
                table = self.relation.as_str()
            )
            .mark_unsupported());
        };
        if let Some(stored) = self.get_row(id).await? {
            return Ok((id.to_string(), stored));
        }
        if let Err(err) = self.insert_keyed(key, id, record).await {
            // A row created concurrently since the check above fails the
            // insert on its unique key; it is then the stored row.
            return match self.get_row(id).await? {
                Some(stored) => Ok((id.to_string(), stored)),
                None => Err(err),
            };
        }
        Ok((id.to_string(), self.read_back(id).await?))
    }

    /// Replace row `id`, creating it when missing.
    ///
    /// A replace is an update of the columns `record` carries: SpacetimeDB has
    /// no `UPSERT`, and delete-then-insert would briefly remove the row from
    /// every live subscription watching it. Columns absent from `record` keep
    /// their stored values, which differs from a full replace only for a
    /// partial record, since SATS rows always carry every column. A row
    /// created concurrently between the existence check and the insert fails
    /// the insert on its unique key, reported as a conflict.
    pub(super) async fn replace_row(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.require(Capability::Update)?;
        let key = self.key_column()?;
        match self.get_row(id).await? {
            None => {
                self.insert_keyed(key, id, record)
                    .await
                    .map_err(|err| err.mark_conflict())?;
                self.read_back(id).await
            }
            Some(stored) => self.update_row(key, id, record, stored).await,
        }
    }

    /// Merge `partial` into row `id`; a row missing from the set is `NotFound`.
    pub(super) async fn patch_row(&self, id: &str, partial: &Rec) -> Result<Rec> {
        self.require(Capability::Update)?;
        let key = self.key_column()?;
        let stored = self.get_row(id).await?.ok_or_else(|| {
            error!("no such row", table = self.relation.as_str(), id = id).mark_not_found()
        })?;
        self.update_row(key, id, partial, stored).await
    }

    pub(super) async fn delete_row(&self, id: &str) -> Result<()> {
        self.require(Capability::Delete)?;
        let key = self.key_column()?;
        let sql = Delete::new(self.relation_ident()?)
            .and_where(self.id_is(key, id)?)
            .where_all(self.narrowing())
            .render();
        self.db.sql(&sql).await.map(|_| ())
    }

    /// Deletes exactly the set: a narrowed shell doesn't empty the table.
    pub(super) async fn delete_rows(&self) -> Result<()> {
        self.require(Capability::DeleteAll)?;
        let sql = Delete::new(self.relation_ident()?)
            .where_all(self.narrowing())
            .render();
        self.db.sql(&sql).await.map(|_| ())
    }

    /// `UPDATE` every declared column of `record` but the key on row `id`,
    /// which is in the set as `stored`, and read the row back.
    async fn update_row(&self, key: &str, id: &str, record: &Rec, stored: Rec) -> Result<Rec> {
        let mut update = Update::new(self.relation_ident()?);
        for (name, value) in record.iter() {
            // An `UPDATE` that moved the key would leave `id` naming nothing;
            // a column the module doesn't declare would fail the statement.
            if name.as_str() == key || !self.metadata.columns.contains_key(name) {
                continue;
            }
            update = update.set(Ident::new(name)?, Literal::from_cbor(value)?);
        }
        if update.is_empty() {
            return Ok(stored);
        }
        let sql = update
            .and_where(self.id_is(key, id)?)
            .where_all(self.narrowing())
            .render();
        self.db.sql(&sql).await?;
        self.get_row(id).await?.ok_or_else(|| {
            error!(
                "the update moved the row out of this set",
                table = self.relation.as_str(),
                id = id
            )
            .mark_conflict()
        })
    }

    /// `INSERT` `record` as row `id`. The id wins over any key value in the
    /// record, typed for its column: a `u64` key handed `'42'` is not that
    /// key, and the host would reject the insert.
    async fn insert_keyed(&self, key: &str, id: &str, record: &Rec) -> Result<()> {
        let mut row = record.clone();
        row.insert(key.to_string(), self.id_as_value(key, id)?);
        let mut insert = Insert::new(self.relation_ident()?);
        for (name, value) in row.iter() {
            // The host rejects a column the module doesn't declare; dropping it
            // keeps a caller's bookkeeping fields from breaking the insert.
            if !self.metadata.columns.contains_key(name) && name.as_str() != key {
                continue;
            }
            insert = insert.set(Ident::new(name)?, Literal::from_cbor(value)?);
        }
        self.db.sql(&insert.render()).await.map(|_| ())
    }

    async fn read_back(&self, id: &str) -> Result<Rec> {
        self.get_row(id).await?.ok_or_else(|| {
            error!(
                "the written row could not be read back through this set",
                table = self.relation.as_str(),
                id = id
            )
        })
    }

    /// The caller's id as a value for its column, by the same rule
    /// `Literal::for_id` renders it into a `WHERE`.
    fn id_as_value(&self, column: &str, id: &str) -> Result<CborValue> {
        let ty = self.column_type(column);
        if ty.is_some_and(|ty| ty.is_integer()) {
            let n = id.parse::<i128>().map_err(|_| {
                error!(
                    "id is not a whole number, but its column is",
                    column = column,
                    id = id
                )
            })?;
            let n = ciborium::value::Integer::try_from(n).map_err(|_| {
                error!(
                    "id does not fit the range CBOR can carry",
                    column = column,
                    id = id
                )
            })?;
            return Ok(CborValue::Integer(n));
        }
        if ty.is_some_and(|ty| ty.is_float()) {
            return id.parse::<f64>().map(CborValue::Float).map_err(|_| {
                error!(
                    "id is not a number, but its column is",
                    column = column,
                    id = id
                )
            });
        }
        Ok(CborValue::Text(id.to_string()))
    }

    /// The key column every single-row write addresses.
    fn key_column(&self) -> Result<&str> {
        self.identity.column().ok_or_else(|| {
            error!(
                "a table with no key has no `WHERE` that identifies a row; method refused",
                table = self.relation.as_str()
            )
            .mark_unsupported()
        })
    }

    /// Refuse a write the shell doesn't advertise before any request is sent,
    /// naming why. Through a `Vista` the write is refused before the shell is
    /// called; this guards a direct call on the shell, which would otherwise
    /// get a server permission error that says nothing about the cause.
    fn require(&self, capability: Capability) -> Result<()> {
        if self.capabilities.has(capability) {
            return Ok(());
        }
        let reason = if !self.db.has_token() {
            "no token is configured, and SpacetimeDB refuses SQL DML from anonymous callers"
        } else {
            "the relation is a view, a keyless table, or SQL DML is restricted to the \
             database owner and this identity is not it; use a reducer"
        };
        Err(error!(
            "cannot write through this relation; method refused",
            capability = capability.as_str(),
            table = self.relation.as_str(),
            reason = reason
        )
        .mark_unsupported())
    }
}
