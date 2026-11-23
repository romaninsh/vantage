//! Narrowing a SQL vista by a native condition, the same for every dialect.

use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_types::Entity;
use vantage_vista::NativeCondition;

use super::Shell;
use super::dialect::SqlShellDialect;

/// Narrow `shell` by a native condition: the dialect's own condition, or the
/// expression the Rhai vocabulary's constructors produce.
pub(crate) fn add_native_condition<T, E>(
    shell: &mut Shell<T, E>,
    condition: NativeCondition,
) -> Result<()>
where
    T: SqlShellDialect<Condition: From<Expression<T::Value>> + 'static>,
    T::Value: 'static,
    E: Entity<T::Value> + 'static,
{
    let condition = match condition.downcast::<T::Condition>() {
        Ok(condition) => condition,
        Err(other) => match other.downcast::<Expression<T::Value>>() {
            Ok(expr) => T::Condition::from(expr),
            Err(other) => {
                return Err(error!(
                    format!(
                        "{} takes its own condition or a query expression as a native condition",
                        T::DRIVER
                    ),
                    key = other.key()
                )
                .mark_incorrect_usage());
            }
        },
    };
    shell.table.add_condition(condition);
    Ok(())
}

/// The script hooks a dialect whose vistas take native conditions shares:
/// `add_native_condition`, the Rhai extensions (its query vocabulary from
/// `$onto`, `with_condition` and the relation vocabulary), and the
/// `where_expr` closure's table and condition. Expands inside an
/// `impl SqlShellDialect for <dialect>` block.
macro_rules! scripted_dialect_hooks {
    ($value:ty, $onto:path) => {
        /// Takes the dialect's own condition or a query expression.
        fn add_native_condition<E>(
            shell: &mut $crate::sql_vista::Shell<Self, E>,
            condition: vantage_vista::NativeCondition,
        ) -> vantage_core::Result<()>
        where
            E: vantage_types::Entity<$value> + 'static,
        {
            $crate::sql_vista::add_native_condition(shell, condition)
        }

        #[cfg(feature = "rhai")]
        fn register_rhai_extensions(engine: &mut vantage_rhai::rhai::Engine) {
            $onto(engine);
            $crate::sql_vista::scripted::register_handle_verbs::<Self>(engine);
        }

        #[cfg(feature = "rhai")]
        fn script_table<E>(
            shell: &$crate::sql_vista::Shell<Self, E>,
        ) -> Option<vantage_rhai::rhai::Dynamic>
        where
            E: vantage_types::Entity<$value> + 'static,
        {
            $crate::sql_vista::scripted::script_table(shell)
        }

        #[cfg(feature = "rhai")]
        fn script_condition(
            expr: vantage_rhai::rhai::Dynamic,
        ) -> vantage_core::Result<vantage_vista::NativeCondition> {
            $crate::sql_vista::scripted::script_condition::<Self>(expr)
        }
    };
}
pub(crate) use scripted_dialect_hooks;
