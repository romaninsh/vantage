//! Narrowing the wrapped table: conditions, native conditions, quicksearch
//! and order. Each clause is checked before it is pushed, so a refused one
//! leaves the shell's query as it was.

use bson::doc;
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_types::Record;
use vantage_vista::{Condition, FilterOp, NativeCondition, SortDirection};

use super::coerce::{self, BsonKind};
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
        let operand = match self.typed_kind(condition) {
            Some(kind) => coerce::declared(field, kind, &condition.value)?,
            None => cbor_to_bson(&condition.value),
        };
        let filter = conditions::op_filter(&path, condition.op, &operand)?;
        self.table.add_condition(filter);
        Ok(())
    }

    /// The kind of a `date`, `timestamp` or `time` column `condition`
    /// compares: its value is typed as a write is, so any form a write takes
    /// compares against the stored DateTime or time text, and a value that
    /// doesn't fit is the write's error. `Like` matches the value's own text.
    fn typed_kind(&self, condition: &Condition) -> Option<BsonKind> {
        let kind = *self.declared.get(condition.column.as_str())?;
        (kind.reads_typed() && condition.op != FilterOp::Like).then_some(kind)
    }

    /// `condition` with its value as a read returns the operand
    /// [`apply_condition`](Self::apply_condition) sends, so rows held in
    /// memory are judged as the database judged them. A condition
    /// `apply_condition` refuses stays as given.
    pub(super) fn typed_condition(&self, condition: &Condition) -> Condition {
        let mut condition = condition.clone();
        let typed = self.typed_kind(&condition).and_then(|kind| {
            let operand = coerce::declared(&condition.column, kind, &condition.value).ok()?;
            coerce::rendered(Some(kind), &operand)
        });
        if let Some(value) = typed {
            condition.value = value;
        }
        condition
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
        let condition = text.map(|text| self.search_condition(text));
        self.current_search_handle = self
            .table
            .replace_temp_condition(self.current_search_handle, condition)?;
        Ok(())
    }

    /// Replace the order; `None` drops it. MongoDB sorts on any field the
    /// table declares.
    pub(super) fn apply_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        let Some((field, dir)) = order else {
            self.table.clear_orders();
            return Ok(());
        };
        self.check_order_column(field)?;
        self.table.clear_orders();
        self.push_order(field, dir)
    }

    /// A tie-breaker after the orders already set: a later key of the sort
    /// document.
    pub(super) fn push_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        self.check_order_column(field)?;
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

    fn check_order_column(&self, field: &str) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for order", field = field));
        }
        Ok(())
    }

    /// Quicksearch filter over [`VistaMetadata::search_columns`], each at its
    /// BSON path.
    ///
    /// [`VistaMetadata::search_columns`]: vantage_vista::VistaMetadata::search_columns
    fn search_condition(&self, text: &str) -> MongoCondition {
        let paths: Vec<String> = self
            .metadata
            .search_columns()
            .into_iter()
            .map(|name| self.dotted_path(name))
            .collect();
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
        table.replace_temp_condition(self.current_search_handle, None)?;
        table
            .get_value(id.clone())
            .await?
            .ok_or_else(|| {
                error!(
                    "inserted row can't be read back through the set",
                    id = id.to_string()
                )
            })
            .and_then(|record| self.unflatten_to_cbor(record))
    }
}
