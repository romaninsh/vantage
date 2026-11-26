//! What [`SqlTableShell`] needs from a dialect: the value codec, the clauses
//! each dialect spells its own way, and the optional surfaces (change feed,
//! native conditions, import, Rhai vocabulary) a dialect may add.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_expressions::Expression;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Entity, InvariantValue, Record};
use vantage_vista::{
    Capability, NativeCondition, Reference, Transaction, Vista, VistaChangeStream, unsupported,
};

use super::shell::SqlTableShell;
use crate::set_writes::SetWrites;

/// The shell type a dialect's hooks receive.
pub(crate) type Shell<T, E> = SqlTableShell<T, E, <T as SqlShellDialect>::Extra>;

#[async_trait]
pub(crate) trait SqlShellDialect:
    TableSource<Id = String, Value: From<CborValue> + InvariantValue> + SetWrites + Send + Sync
{
    /// State the shell carries beyond its table, capabilities and metadata.
    type Extra: Clone + Send + Sync + 'static;

    /// The driver label, also the dialect a [`Preview`](vantage_vista::Preview)
    /// names.
    const DRIVER: &'static str;

    /// A CBOR cell as the dialect's value, without a type tag.
    fn cell(value: CborValue) -> Self::Value;

    fn cbor(value: Self::Value) -> CborValue;

    /// This datasource running its statements on `tx`.
    fn in_transaction(&self, tx: &Transaction) -> Result<Self>;

    /// The SELECT the table would run, rendered with values inline; the
    /// error of an expression column that can't be built.
    fn preview<E: Entity<Self::Value>>(table: &Table<Self, E>) -> Result<String>;

    /// A stored column as a condition's subject.
    fn column_expr(column: &Self::Column<Self::AnyType>) -> Expression<Self::Value>;

    /// `subject <op> value`, `template` being the binary operator's
    /// `"{} = {}"`-style template. The subject is the column, or an
    /// expression column's expression ([`where_subject`](crate::where_subject)).
    fn compare(
        subject: Expression<Self::Value>,
        template: &str,
        value: Self::Value,
    ) -> Self::Condition;

    /// `subject` NULL-safely equal to `value` (`None` is NULL), or not when
    /// `negate`.
    fn is(
        subject: Expression<Self::Value>,
        value: Option<Self::Value>,
        negate: bool,
    ) -> Self::Condition;

    /// `subject IN (values)`, or `NOT IN` when `negate`.
    fn in_list(
        subject: Expression<Self::Value>,
        values: &[Self::Value],
        negate: bool,
    ) -> Self::Condition;

    /// `subject` matched against a `LIKE` pattern that escapes with `$`.
    fn like(subject: Expression<Self::Value>, pattern: String) -> Self::Condition;

    /// `field` as an ORDER BY term.
    fn order_term(field: &str) -> Self::Condition;

    /// A clone of `table` for a windowed read, ordered as the dialect needs
    /// for successive windows to neither repeat nor skip a row.
    fn ordered_for_paging<E: Entity<Self::Value>>(table: &Table<Self, E>) -> Table<Self, E>;

    /// Turns a traversal target into a vista, built by a factory that
    /// carries what `shell` knows of its own.
    fn target_factory<E>(
        shell: &Shell<Self, E>,
    ) -> impl Fn(Table<Self, EmptyEntity>) -> Result<Vista> + Send + Sync + 'static
    where
        E: Entity<Self::Value> + 'static;

    /// The target of the joined `reference`, narrowed to `row` when there is
    /// one. A dialect that builds some targets another way overrides this.
    fn join_target<E>(
        shell: &Shell<Self, E>,
        relation: &str,
        reference: &Reference,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista>
    where
        E: Entity<Self::Value> + 'static,
    {
        shell.join_target(relation, reference, row)
    }

    fn watch<E>(_shell: &Shell<Self, E>) -> Result<VistaChangeStream>
    where
        E: Entity<Self::Value> + 'static,
    {
        unsupported(Capability::Subscribe)
    }

    fn add_native_condition<E>(
        _shell: &mut Shell<Self, E>,
        _condition: NativeCondition,
    ) -> Result<()>
    where
        E: Entity<Self::Value> + 'static,
    {
        unsupported(Capability::NativeConditions)
    }

    async fn import<E>(
        _shell: &Shell<Self, E>,
        _records: &IndexMap<String, Record<CborValue>>,
    ) -> Result<usize>
    where
        E: Entity<Self::Value> + 'static,
    {
        unsupported(Capability::Import)
    }

    /// Backend vocabulary for a Rhai engine. A dialect without an expression
    /// vocabulary registers nothing.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(_engine: &mut vantage_rhai::rhai::Engine) {}

    /// The shell's table as a `where_expr` closure's `table`; a dialect
    /// without the relation vocabulary has none.
    #[cfg(feature = "rhai")]
    fn script_table<E>(_shell: &Shell<Self, E>) -> Option<vantage_rhai::rhai::Dynamic>
    where
        E: Entity<Self::Value> + 'static,
    {
        None
    }

    /// A `where_expr` closure's expression as a native condition.
    #[cfg(feature = "rhai")]
    fn script_condition(_expr: vantage_rhai::rhai::Dynamic) -> Result<NativeCondition> {
        unsupported(Capability::NativeConditions)
    }
}
