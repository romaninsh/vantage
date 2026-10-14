//! Keeping a write inside a set: the one rule a Dio's
//! [`flash`](crate::Dio::flash) and the facade's writes both apply.
//!
//! A set is a query's conditions and search. A record written into it gets
//! the set's equality conditions on flat cells filled in, and must then
//! satisfy the whole query; a row the set doesn't hold may not be taken over.
//! Every refusal is a `Conflict` naming the table.
//!
//! On an augmented Dio a clause may name a value only the detail pass fills.
//! A row already held is judged with its augmented values, so such a clause
//! judges its real value; the caller hydrates a cached row, or augments the
//! master's row in memory, first. A record being written carries no
//! augmented values, so it is judged by the clauses it can carry (see
//! [`DioInner::without_augmented`]), and the set's equality conditions on
//! augmented columns are never filled into it: the master has never heard
//! of those columns.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;
use vantage_vista::{FilterOp, VistaMetadata, VistaQuery};

use crate::dio::DioInner;

/// A query read as the set a write must stay inside.
#[derive(Clone)]
pub(crate) struct SetRule<'a> {
    /// What a held row is judged by.
    query: &'a VistaQuery,
    /// What a written record is judged by: `query` without the clauses on
    /// augmented values.
    written: VistaQuery,
    dio: &'a DioInner,
    metadata: &'a VistaMetadata,
    table: &'a str,
}

impl<'a> SetRule<'a> {
    /// `query` as a set over `dio`'s rows.
    pub(crate) fn new(query: &'a VistaQuery, dio: &'a DioInner) -> Self {
        Self {
            query,
            written: dio.without_augmented(query),
            dio,
            metadata: &dio.metadata,
            table: &dio.cache_table_name,
        }
    }

    /// Whether a held row must carry its augmented values before this rule
    /// judges it: hydrated from the cache, or, for the master's row on a
    /// cache miss, augmented in memory.
    pub(crate) fn needs_hydration(&self) -> bool {
        self.dio.augmented_clause(self.query).is_some()
    }

    /// Whether the set leaves rows out: it has a condition, a native
    /// condition or a search. An order alone doesn't.
    pub(crate) fn narrows(&self) -> bool {
        !self.query.conditions.is_empty()
            || !self.query.native.is_empty()
            || self.query.search.is_some()
    }

    /// Refuse every write when the set holds a native condition: it can't be
    /// judged in memory, so no write could be shown to stay inside the set.
    /// Only a Dio with a Lens writer checks its native clauses here; through
    /// the master, the master confines them itself.
    pub(crate) fn refuse_native(&self) -> Result<()> {
        if self.query.native.is_empty() {
            return Ok(());
        }
        Err(error!(
            "a write through the Lens writer can't be kept inside this Dio: its native condition can't be judged in memory",
            table = self.table,
            native = self.query.native.join(", ")
        )
        .mark_unsupported())
    }

    /// Whether `row` is in the set. A clause the evaluator can't judge (a
    /// native condition) is an error, never a pass.
    pub(crate) fn admits(&self, row: &Record<CborValue>) -> Result<bool> {
        self.query.matches(row, self.metadata)
    }

    /// The set's equality conditions on flat cells, as record invariants. A
    /// dotted path into a nested map has no flat cell to fill, so such a write
    /// is left to [`admits`](Self::admits); a column the schema declares with
    /// a dot in its name is a flat cell. A condition on an augmented column
    /// is no invariant: the detail pass fills that cell, never a write.
    pub(crate) fn invariants(&self) -> IndexMap<String, CborValue> {
        self.written
            .conditions
            .iter()
            .filter(|c| c.op == FilterOp::Eq)
            .filter(|c| !c.column.contains('.') || self.metadata.columns.contains_key(&c.column))
            .map(|c| (c.column.clone(), c.value.clone()))
            .collect()
    }

    /// `record` with the set's invariants filled in, or `Conflict` when it
    /// is still outside the set (or contradicts an invariant). It is judged
    /// by the clauses a written record can carry.
    pub(crate) fn conform(&self, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        let mut out = record.clone();
        vantage_dataset::invariants::conform(&mut out, &self.invariants())?;
        if !self.written.matches(&out, self.metadata)? {
            return Err(
                error!("record does not belong to this set", table = self.table).mark_conflict(),
            );
        }
        Ok(out)
    }

    /// Refuse a patch that changes an invariant, or whose result, `fields`
    /// merged over `before`, leaves the set. `before` is a held row, so it
    /// carries its augmented values when the caller hydrated it.
    pub(crate) fn check_patch(
        &self,
        id: &str,
        before: &Record<CborValue>,
        fields: &Record<CborValue>,
    ) -> Result<()> {
        vantage_dataset::invariants::validate(fields, &self.invariants())?;
        let mut merged = before.clone();
        merged.extend(fields.iter().map(|(k, v)| (k.clone(), v.clone())));
        if !self.admits(&merged)? {
            return Err(error!(
                "patch would move the row out of the set",
                table = self.table,
                id = id
            )
            .mark_conflict());
        }
        Ok(())
    }

    /// `row`, the one held under `id`, unless it is outside the set: then a
    /// write under `id` would take over a row the set doesn't hold, which is
    /// `Conflict`.
    pub(crate) fn not_outside(
        &self,
        id: &str,
        row: Option<Record<CborValue>>,
    ) -> Result<Option<Record<CborValue>>> {
        match row {
            Some(row) if !self.admits(&row)? => Err(self.outside(id)),
            row => Ok(row),
        }
    }

    fn outside(&self, id: &str) -> VantageError {
        error!(
            "id is held by a row outside this set",
            table = self.table,
            id = id
        )
        .mark_conflict()
    }
}
