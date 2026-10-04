//! Wrapper shells: [`ForwardShell`] hands every [`TableShell`](crate::TableShell)
//! method to an inner shell unless the wrapper overrides it, and
//! [`forward_table_shell!`](crate::forward_table_shell) implements it for a
//! wrapper holding the inner shell in a field.

mod blanket;
mod shell;
#[cfg(test)]
mod tests;

pub use shell::ForwardShell;

/// Implement [`ForwardShell`] (and so [`TableShell`](crate::TableShell)) for
/// `$ty`, whose `$field` holds the inner shell (anything that derefs to a
/// `TableShell`).
///
/// The block holds the rest of the impl, written as in an `#[async_trait]`
/// impl: `clone_shell` (required), then whichever methods the wrapper
/// changes. Every other method answers as the inner shell does.
///
/// ```ignore
/// vantage_vista::forward_table_shell!(Logged, inner, {
///     fn clone_shell(&self) -> Option<Box<dyn TableShell>> { /* re-wrap */ }
///     fn columns(&self) -> &IndexMap<String, Column> { &self.columns }
///     // reads, writes, get_ref...
/// });
/// ```
#[macro_export]
macro_rules! forward_table_shell {
    ($ty:ty, $field:ident, { $($rest:tt)* }) => {
        #[$crate::__private::async_trait]
        impl $crate::ForwardShell for $ty {
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
