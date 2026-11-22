//! `SurrealTableShell`: a typed `Table<SurrealDB, E>` behind the `TableShell`
//! boundary. It stays generic in `E` so `with_expression` closures survive
//! the wrap; a clone copies the query state and shares the connection. Ids
//! cross as `table:id` text, and a bare id names a record of this table.

mod narrow;
mod refs;
#[cfg(feature = "rhai")]
mod scripted;
mod watch;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::Inserted;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_expressions::traits::selectable::Selectable;
use vantage_table::conditions::ConditionHandle;
use vantage_table::pagination::Pagination;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity, Record, rfc3339_to_tag12, temporal};
use vantage_vista::{
    Condition, NativeCondition, Page, Preview, SortDirection, TableShell, Transaction, Vista,
    VistaCapabilities, VistaChangeStream, VistaMetadata, direct,
};

use crate::surrealdb::SurrealDB;
use crate::surrealdb::declared::declared_name;
use crate::thing::Thing;
use crate::types::AnySurrealType;
use crate::vista::factory::SurrealVistaFactory;

#[cfg(feature = "rhai")]
pub(crate) use scripted::register_extensions;
#[cfg(feature = "rhai")]
pub use scripted::register_rule_conditions;

/// The `TableShell` a SurrealDB vista wraps.
#[derive(Clone)]
pub struct SurrealTableShell<E = EmptyEntity>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) table: Table<SurrealDB, E>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Handle for the active quicksearch condition, removed when the search
    /// is replaced or cleared.
    pub(crate) current_search_handle: Option<ConditionHandle>,
    /// The factory that built this shell, with its spec resolver and build
    /// injector, for the traversal targets and scripts it builds in turn.
    pub(crate) factory: SurrealVistaFactory,
}

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType>,
{
    pub(crate) fn new(
        table: Table<SurrealDB, E>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
        factory: SurrealVistaFactory,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            current_search_handle: None,
            factory,
        }
    }

    /// A Vista-side id as a `Thing`: `table:id` parses as it is, and a bare
    /// id takes this table's name, so `"biff"` and `"client:biff"` agree.
    fn parse_id(&self, id: &str) -> Thing {
        if id.contains(':') {
            id.parse::<Thing>()
                .unwrap_or_else(|_| Thing::new(self.table.table_name(), id))
        } else {
            Thing::new(self.table.table_name(), id)
        }
    }
}

/// A row read from `table`, its dates, times and instants in the carrier
/// forms of [`temporal`], each by its column's declared type. A `date`,
/// `time` or `timestamp` column holding something that isn't one is an error
/// naming the column and the value.
fn to_cbor_record<E: Entity<AnySurrealType>>(
    table: &Table<SurrealDB, E>,
    record: Record<AnySurrealType>,
) -> Result<Record<CborValue>> {
    record
        .into_iter()
        .map(|(k, v)| {
            let kind = declared_kind(table, &k);
            let value = read_cell(table.table_name(), &k, kind, v.into_value())?;
            Ok((k, value))
        })
        .collect()
}

/// `value` of `column` as a read returns it; see [`to_cbor_record`].
fn read_cell(table: &str, column: &str, kind: Option<&str>, value: CborValue) -> Result<CborValue> {
    if !is_temporal(kind) {
        return Ok(instants(value));
    }
    temporal_cell(kind, &value).ok_or_else(|| {
        let expected = kind.unwrap_or_default();
        let held = match &value {
            CborValue::Tag(temporal::TAG_SURREAL_INSTANT, _) => temporal::as_instant(&value)
                .map(|at| format!("d'{}'", temporal::format_instant(&at))),
            CborValue::Text(s) => Some(format!("{s:?}")),
            _ => None,
        }
        .unwrap_or_else(|| AnySurrealType::from(value.clone()).to_string());
        error!(
            format!("`{column}` of `{table}` holds {held}, which is not a `{expected}`"),
            table = table,
            column = column,
            expected = expected,
            value = held
        )
    })
}

/// The YAML type (`date`, `timestamp`, …) `column` of `table` is declared as.
fn declared_kind<E: Entity<AnySurrealType>>(
    table: &Table<SurrealDB, E>,
    column: &str,
) -> Option<&'static str> {
    table
        .columns()
        .get(column)
        .and_then(|c| declared_name(c.get_type()))
}

/// `value` of a `date`, `time` or `timestamp` column in its carrier form: a
/// `date` column's midnight datetime or date text reads as a date, a
/// `timestamp` column's datetime or instant text as an instant, a `time`
/// column's text as a time. No value (null, `NONE`, `""`) is null. `None`
/// when the value isn't of the column's kind (a date column's datetime at
/// 10:00, text that doesn't parse), or `kind` isn't one of the three.
fn temporal_cell(kind: Option<&str>, value: &CborValue) -> Option<CborValue> {
    if !is_temporal(kind) {
        return None;
    }
    match (kind, value) {
        (_, CborValue::Null | CborValue::Tag(6, _)) => Some(CborValue::Null),
        (_, CborValue::Text(s)) if s.is_empty() => Some(CborValue::Null),
        (_, CborValue::Array(items)) => items
            .iter()
            .map(|v| temporal_cell(kind, v))
            .collect::<Option<Vec<_>>>()
            .map(CborValue::Array),
        (Some("date"), v) => temporal::as_date(v)
            .or_else(|| temporal::midnight_date(v))
            .map(|day| temporal::date(&day)),
        (Some("time"), v) => temporal::as_time(v).map(|t| temporal::time(&t)),
        (Some("timestamp"), v) => temporal::as_instant(v).map(|at| temporal::instant(&at)),
        _ => None,
    }
}

/// Every datetime in `value`, nested ones too, as a carrier instant.
fn instants(value: CborValue) -> CborValue {
    match value {
        CborValue::Array(items) => CborValue::Array(items.into_iter().map(instants).collect()),
        CborValue::Map(entries) => {
            CborValue::Map(entries.into_iter().map(|(k, v)| (k, instants(v))).collect())
        }
        CborValue::Tag(temporal::TAG_SURREAL_INSTANT, inner) => {
            let tag12 = CborValue::Tag(temporal::TAG_SURREAL_INSTANT, inner);
            temporal::as_instant(&tag12)
                .map(|at| temporal::instant(&at))
                .unwrap_or(tag12)
        }
        other => other,
    }
}

/// A condition operand of a column declared `kind`, in the form its rows
/// read in. An operand that doesn't fit the column stays as it is: it
/// matches nothing.
fn comparable(kind: Option<&str>, value: CborValue) -> CborValue {
    if !is_temporal(kind) {
        return instants(value);
    }
    temporal_cell(kind, &value).unwrap_or_else(|| instants(value))
}

fn is_temporal(kind: Option<&str>) -> bool {
    matches!(kind, Some("date" | "time" | "timestamp"))
}

fn to_native_record(record: &Record<CborValue>) -> Record<AnySurrealType> {
    record
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                AnySurrealType::from(datetimes_as_tag12(v.clone())),
            )
        })
        .collect()
}

/// SurrealDB refuses `Tag(0, rfc3339)` (Rhai's `now()`, the audit logger's
/// `at`) and takes only `Tag(12, [seconds, nanos])`. `Table` types declared
/// datetime columns itself (`coerce_column_value`); this covers the rest: a
/// column without a datetime type (a host that still declares timestamps as
/// `string`), and datetimes nested in objects and arrays. Text that isn't RFC
/// 3339 is left for the server to reject.
///
/// SurrealDB has no tag for a date or a time of day either: a carrier date
/// is written as its midnight UTC, a carrier time as its text.
fn datetimes_as_tag12(value: CborValue) -> CborValue {
    match value {
        CborValue::Tag(0, inner) => match *inner {
            CborValue::Text(s) => rfc3339_to_tag12(&s)
                .unwrap_or_else(|| CborValue::Tag(0, Box::new(CborValue::Text(s)))),
            other => CborValue::Tag(0, Box::new(other)),
        },
        CborValue::Tag(temporal::TAG_DATE | temporal::TAG_LEGACY_DATE, _) => {
            match temporal::as_date(&value) {
                Some(day) => AnySurrealType::new(day).into_value(),
                None => value,
            }
        }
        CborValue::Tag(temporal::TAG_TIME, _) => match temporal::as_time(&value) {
            Some(time) => CborValue::Text(temporal::format_time(&time)),
            None => value,
        },
        CborValue::Array(items) => {
            CborValue::Array(items.into_iter().map(datetimes_as_tag12).collect())
        }
        CborValue::Map(entries) => CborValue::Map(
            entries
                .into_iter()
                .map(|(k, v)| (k, datetimes_as_tag12(v)))
                .collect(),
        ),
        // Other tags (a record id, a decimal) are left whole: nothing inside
        // them is rewritten.
        other => other,
    }
}

fn to_cbor_rows<E: Entity<AnySurrealType>>(
    table: &Table<SurrealDB, E>,
    raw: IndexMap<Thing, Record<AnySurrealType>>,
) -> Result<IndexMap<String, Record<CborValue>>> {
    raw.into_iter()
        .map(|(thing, record)| Ok((thing.to_string(), to_cbor_record(table, record)?)))
        .collect()
}

#[async_trait]
impl<E> TableShell for SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "surrealdb"
    }

    /// The SurrealQL SELECT as it stands, with every condition and order
    /// rendered inline. A fetched window's `LIMIT`/`START` is added at fetch
    /// time, so it is absent here; `preview_window` renders one.
    /// A read that an expression column fails has no query: the note
    /// carries the error the read would return.
    fn preview(&self) -> Preview {
        self.preview_window(0, None)
    }

    fn preview_window(&self, offset: usize, limit: Option<usize>) -> Preview {
        let mut select = match self.table.try_select() {
            Ok(select) => select,
            Err(e) => return Preview::new().with_note(format!("the query can't be built: {e}")),
        };
        // No window asked for: the read keeps the table's own pagination.
        if offset > 0 || limit.is_some() {
            select.set_limit(
                limit.map(|n| n as i64),
                (offset > 0).then_some(offset as i64),
            );
        }
        Preview::new().with_query(select.preview())
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        to_cbor_rows(&self.table, self.table.list_values().await?)
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let record = self.table.get_value(self.parse_id(id)).await?;
        record.map(|r| to_cbor_record(&self.table, r)).transpose()
    }

    async fn count(&self) -> Result<i64> {
        self.table.get_count().await
    }

    /// `LIMIT limit START offset`, applied to a clone so the shell's query
    /// is left as it was.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let mut window = self.table.clone();
        window.set_pagination(Some(Pagination::window(offset as i64, limit as i64)));
        let rows = to_cbor_rows(&self.table, window.list_values().await?)?;
        Ok(Page::new(rows.into_iter().collect()))
    }

    async fn watch(&self) -> Result<VistaChangeStream> {
        self.table.check_read().await?;
        self.live_changes().await
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.apply_condition(condition)
    }

    fn comparable_condition(&self, condition: &Condition) -> Condition {
        self.typed_condition(condition)
    }

    fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        self.apply_native_condition(condition)
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.apply_search(text)
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.apply_order(order)
    }

    fn add_order(&mut self, column: &str, dir: SortDirection) -> Result<()> {
        self.push_order(column, dir)
    }

    async fn insert(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
    ) -> Result<(String, Inserted<CborValue>)> {
        let native = to_native_record(record);
        let Some(id) = id else {
            let thing = self.table.insert_return_id_value(&native).await?;
            let stored = self.read_back(&thing).await?;
            return Ok((thing.to_string(), Inserted::Created(stored)));
        };
        let thing = self.parse_id(id);
        let inserted = match self.table.insert_value(thing.clone(), &native).await? {
            Inserted::Created(stored) => Inserted::Created(to_cbor_record(&self.table, stored)?),
            Inserted::Existed(stored) => Inserted::Existed(to_cbor_record(&self.table, stored)?),
            Inserted::Accepted => Inserted::Accepted,
        };
        Ok((thing.to_string(), inserted))
    }

    async fn replace(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        let replaced = self
            .table
            .replace_value(self.parse_id(id), &to_native_record(record))
            .await?;
        to_cbor_record(&self.table, replaced)
    }

    /// An insert of a missing row, a replace of a stored one, each with its
    /// own hooks. Without them it is the replace statement, an
    /// `UPSERT … CONTENT` that creates a missing row in one operation.
    async fn upsert(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        let upserted = self
            .table
            .upsert_value(self.parse_id(id), &to_native_record(record))
            .await?;
        to_cbor_record(&self.table, upserted)
    }

    async fn patch(&self, id: &str, partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        let patched = self
            .table
            .patch_value(self.parse_id(id), &to_native_record(partial))
            .await?;
        to_cbor_record(&self.table, patched)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.table.delete(self.parse_id(id)).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.table.delete_all().await
    }

    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        self.ref_target(relation, row)
    }

    /// The table keeps its hooks, conditions and references; only its
    /// datasource runs in the transaction. A datasource without interactive
    /// transactions writes directly.
    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        let source = self.table.data_source();
        if !source.supports_transactions() {
            return Ok(direct(Box::new(self.clone())));
        }
        let source = source.in_transaction(tx)?;
        Ok(Box::new(Self {
            table: self.table.with_data_source(source),
            ..self.clone()
        }))
    }

    /// SurrealDB's expression vocabulary under `surreal::` (with the
    /// `surreal::me` anchor) and `self.with_condition(<expr>)`; see
    /// `register_extensions`.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        register_extensions(engine);
    }
}
