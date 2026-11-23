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
