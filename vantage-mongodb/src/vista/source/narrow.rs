//! Narrowing the wrapped table: conditions, native conditions, quicksearch
//! and order. Each clause is checked before it is pushed, so a refused one
//! leaves the shell's query as it was.

use bson::doc;
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::traits::table_source::TableSource;
use vantage_types::Record;
use vantage_vista::{ColumnFlag, Condition, FilterOp, NativeCondition, SortDirection};

use super::{MongoTableShell, conditions};
use crate::condition::MongoCondition;
use crate::id::MongoId;
use crate::mongodb::impls::joins::join_alias_of;
use crate::vista::cbor::cbor_to_bson;

impl MongoTableShell {
    /// Push `column <op> value` as a filter document on the column's BSON
    /// path. Operands that look like ids match both spellings (see the
    /// `conditions` module).
    ///
    /// The column must be one the table declares. A shell that reads
    /// documents as stored (no path map: a bare collection, a column-less
    /// spec, or a traversal target the resolver doesn't know) declares only
    /// `_id`, so it takes equality on any top-level field. That is how a
    /// traversal or a script's `where(..)` narrows such a set by a field.
    ///
    /// A dotted path (`address.city`) whose root is a declared column is
    /// evaluated natively: MongoDB's dot notation walks into the root's
    /// stored document.
    pub(super) fn apply_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        let as_stored = self.column_paths.is_empty() && condition.op == FilterOp::Eq;
        let path = if as_stored || self.table.columns().contains_key(field) {
            self.dotted_path(field)
        } else {
            self.nested_path(field)
                .ok_or_else(|| error!("Unknown column for condition", field = field))?
        };
        let filter = conditions::op_filter(&path, condition.op, &cbor_to_bson(&condition.value))?;
        self.table.add_condition(filter);
        Ok(())
    }

    /// The BSON path of `root.rest`, when `root` is a declared column: the
    /// root's own path with `rest` appended.
    fn nested_path(&self, field: &str) -> Option<String> {
        let (root, rest) = field.split_once('.')?;
        self.table
            .columns()
            .contains_key(root)
            .then(|| format!("{}.{rest}", self.dotted_path(root)))
    }

    /// Takes a `bson::Document` or a `MongoCondition`. Field names are raw
    /// BSON paths, not spec column names.
    pub(super) fn apply_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        self.table
            .add_condition(conditions::native_condition(condition)?);
        Ok(())
    }

    /// Replace the quicksearch condition; `None` drops it.
    pub(super) fn apply_search(&mut self, text: Option<&str>) -> Result<()> {
        if let Some(handle) = self.current_search_handle {
            self.table.temp_remove_condition(handle)?;
            self.current_search_handle = None;
        }
        if let Some(text) = text {
            let condition = self.search_condition(text);
            self.current_search_handle = Some(self.table.temp_add_condition(condition));
        }
        Ok(())
    }

    /// Replace the order; `None` drops it. MongoDB sorts on any field the
    /// table declares.
    pub(super) fn apply_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        let Some((field, dir)) = order else {
            self.table.clear_orders();
            return Ok(());
        };
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for order", field = field));
        }
        self.table.clear_orders();
        // `select_from_table` reads the field name from the first key of the
        // OrderBy expression's document; the value is ignored, and the
        // direction comes from `OrderBy::direction`.
        let dotted = self.dotted_path(field);
        self.table.add_order(OrderBy {
            expression: MongoCondition::Doc(doc! { dotted: 1 }),
            direction: match dir {
                SortDirection::Ascending => TableSortDirection::Ascending,
                SortDirection::Descending => TableSortDirection::Descending,
            },
        });
        Ok(())
    }

    /// Quicksearch filter over the columns flagged `searchable`. A shell over
    /// a typed table that flags none searches every column instead, matching
    /// `MongoDB::search_table_condition`.
    fn search_condition(&self, text: &str) -> MongoCondition {
        let paths: Vec<String> = self
            .metadata
            .columns
            .iter()
            .filter(|(_, col)| col.has_flag(ColumnFlag::Searchable))
            .map(|(name, _)| self.dotted_path(name))
            .collect();
        if paths.is_empty() && self.search_unflagged_columns {
            return self
                .table
                .data_source()
                .search_table_condition(&self.table, text);
        }
        conditions::search_filter(&paths, text).into()
    }

    /// Dotted form of a spec column's BSON path, used for Mongo filters and
    /// sorts; a joined column's computed alias. Falls back to the spec name
    /// when the column isn't in the path map.
    pub(super) fn dotted_path(&self, field: &str) -> String {
        if let Some(alias) = join_alias_of(&self.table, field) {
            return alias;
        }
        match self.column_paths.get(field) {
            Some(path) => path.join("."),
            None => field.to_string(),
        }
    }

    /// The row just inserted as `id`, read with the quicksearch lifted: a new
    /// row conforms to the set's conditions, but needn't match the search
    /// text. A row that still can't be read (one outside a native condition,
    /// say) is an error.
    pub(super) async fn read_back(&self, id: &MongoId) -> Result<Record<CborValue>> {
        let mut table = self.table.clone();
        if let Some(handle) = self.current_search_handle {
            table.temp_remove_condition(handle)?;
        }
        table
            .get_value(id.clone())
            .await?
            .map(|record| self.unflatten_to_cbor(record))
            .ok_or_else(|| {
                error!(
                    "inserted row can't be read back through the set",
                    id = id.to_string()
                )
            })
    }
}
