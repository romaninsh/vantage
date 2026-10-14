//! How the mock reads its conditions: literal equalities, the search it
//! builds, the subquery a relation filters by, and the write-contract errors.

use serde_json::Value;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    Expression, Expressive, Selectable, SelectableDataSource, traits::expressive::ExpressiveEnum,
};
use vantage_types::{Entity, InvariantValue, Record};

use super::MockTableSource;
use crate::{table::Table, traits::column_like::ColumnLike};

/// The search condition of a table with no column to search.
const SEARCH_NOTHING: &str = "FALSE";

impl MockTableSource {
    /// Whether `row` satisfies every condition of `table`.
    ///
    /// Evaluates literal `column = value` conditions and the
    /// `(a LIKE '%…%' OR b LIKE '%…%')` search that
    /// [`TableSource::search_table_condition`](crate::traits::table_source::TableSource::search_table_condition)
    /// produces; any other condition is refused with `Unsupported` rather
    /// than matching every row.
    pub(super) fn row_in_set<E: Entity<Value>>(
        table: &Table<Self, E>,
        row: &Record<Value>,
    ) -> Result<bool> {
        for condition in table.conditions() {
            let matched =
                if let Some((column, expected)) = crate::conditions::literal_equality(condition) {
                    row.get(&column).is_some_and(|v| v.value_eq(&expected))
                } else if let Some(branches) = Self::search_branches(condition) {
                    branches.iter().any(|(column, needle)| {
                        row.get(column)
                            .and_then(Value::as_str)
                            .is_some_and(|text| text.to_lowercase().contains(needle))
                    })
                } else {
                    return Err(vantage_core::error!(
                        "mock table source cannot evaluate condition",
                        condition = condition.preview()
                    )
                    .mark_unsupported());
                };
            if !matched {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// One `column LIKE '%value%'` per search column, OR-ed together, or
    /// `FALSE` for a table without columns.
    pub(super) fn search_condition<E: Entity<Value>>(
        table: &Table<Self, E>,
        search_value: &str,
    ) -> Expression<Value> {
        let pattern = Value::String(format!("%{}%", search_value));
        let mut branches: Vec<Expression<Value>> = table
            .search_columns()
            .into_iter()
            .map(|column| {
                Expression::new(
                    format!("{} LIKE {{}}", column.name()),
                    vec![ExpressiveEnum::Scalar(pattern.clone())],
                )
            })
            .collect();
        match branches.len() {
            0 => Expression::new(SEARCH_NOTHING, vec![]),
            1 => branches.remove(0),
            n => Expression::new(
                format!("({})", vec!["{}"; n].join(" OR ")),
                branches.into_iter().map(ExpressiveEnum::Nested).collect(),
            ),
        }
    }

    /// The `(column, lowercased needle)` branches of a search condition built
    /// by `search_condition`: one `column LIKE {}`, an OR group of them, or
    /// `FALSE` (no branch) for a table without columns.
    fn search_branches(condition: &Expression<Value>) -> Option<Vec<(String, String)>> {
        if condition.template == SEARCH_NOTHING {
            return Some(Vec::new());
        }
        if let Some(column) = condition.template.strip_suffix(" LIKE {}") {
            return match condition.parameters.as_slice() {
                [ExpressiveEnum::Scalar(Value::String(pattern))] => Some(vec![(
                    column.to_string(),
                    pattern.trim_matches('%').to_lowercase(),
                )]),
                _ => None,
            };
        }
        let group = condition.template.strip_prefix('(')?.strip_suffix(')')?;
        if group.split(" OR ").any(|part| part != "{}") {
            return None;
        }
        let mut branches = Vec::new();
        for parameter in &condition.parameters {
            let ExpressiveEnum::Nested(branch) = parameter else {
                return None;
            };
            branches.extend(Self::search_branches(branch)?);
        }
        Some(branches)
    }

    /// `SELECT column FROM table WHERE <table's conditions>`, through the
    /// select source.
    pub(super) fn column_subquery<E: Entity<Value>>(
        &self,
        table: &Table<Self, E>,
        column: &str,
    ) -> Expression<Value> {
        let mut select = self.select();
        select.add_source(table.table_name(), None);
        select.clear_fields();
        select.add_field(column);
        for condition in table.conditions() {
            select.add_where_condition(condition.clone());
        }
        select.expr()
    }

    /// Refuse a by-id write up front when `table` holds a condition that is
    /// not a literal equality, so a missing row is refused rather than reported
    /// as absent.
    pub(super) fn ensure_confinable<E: Entity<Value>>(table: &Table<Self, E>) -> Result<()> {
        if table
            .conditions()
            .all(|c| crate::conditions::literal_equality(c).is_some())
        {
            return Ok(());
        }
        Err(vantage_core::error!(
            "MockTableSource confines writes only for literal equality conditions"
        )
        .mark_unsupported())
    }

    pub(super) fn outside(id: &str) -> vantage_core::VantageError {
        vantage_core::error!("id is held by a row outside this set", id = id).mark_conflict()
    }

    pub(super) fn misfit(id: &str) -> vantage_core::VantageError {
        vantage_core::error!("record does not belong to this set", id = id).mark_conflict()
    }
}
