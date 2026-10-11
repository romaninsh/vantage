//! Wrapper shells: [`forward_table_shell!`](crate::forward_table_shell)
//! implements [`TableShell`](crate::TableShell) for a wrapper holding an
//! inner shell in a field, handing every method to it unless the wrapper
//! overrides it.

mod blanket;
mod shell;
#[cfg(test)]
mod tests;

pub use shell::ForwardShell;

/// Implement [`TableShell`](crate::TableShell) for `$ty`, whose `$field`
/// holds the inner shell (anything that derefs to a `TableShell`, usually a
/// `Box<dyn TableShell>`). `$ty` must be `Clone`; its clone carries the inner
/// shell's clone, narrowing included.
///
/// The block holds the methods the wrapper changes, written as in an
/// `#[async_trait]` impl; every other method answers as the inner shell
/// does. Inside the block a call to the wrapper's own version of a method is
/// spelled `TableShell::list(self)`.
///
/// ```ignore
/// #[derive(Clone)]
/// struct Logged { inner: Box<dyn TableShell>, columns: IndexMap<String, Column> }
///
/// vantage_vista::forward_table_shell!(Logged, inner, {
///     fn columns(&self) -> &IndexMap<String, Column> { &self.columns }
///     // reads, writes, get_ref...
/// });
///
/// let wrapped = vista.map_shell(|inner| Logged { inner, columns });
/// ```
#[macro_export]
macro_rules! forward_table_shell {
    ($ty:ty, $field:ident, { $($rest:tt)* }) => {
        #[$crate::__private::async_trait]
        impl $crate::__private::ForwardShell for $ty {
            fn inner(&self) -> &dyn $crate::TableShell {
                &*self.$field
            }
            fn inner_mut(&mut self) -> &mut dyn $crate::TableShell {
                &mut *self.$field
            }

            $($rest)*
        }
    };
}
