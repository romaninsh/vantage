//! [`forward_table_shell!`](crate::forward_table_shell): a [`TableShell`](crate::TableShell)
//! impl for a wrapper shell that hands every read to an inner shell and
//! writes only what it changes.

/// Implement [`TableShell`](crate::TableShell) for `$ty`, forwarding the schema,
/// reads, narrowing, `contained` refs, `watch_vista` and the Rhai hooks to
/// `self.$field` (anything that derefs to a `TableShell`).
///
/// The block holds the rest of the impl, written as in an `#[async_trait]`
/// impl: `capabilities`, `clone_shell`, `get_ref` and `get_ref_target` (a
/// wrapper usually re-wraps what these return), plus whichever writes the
/// wrapper changes. Writes left out keep the trait defaults.
///
/// ```ignore
/// vantage_vista::forward_table_shell!(Logged, inner, {
///     fn capabilities(&self) -> &VistaCapabilities { self.inner.capabilities() }
///     // clone_shell, get_ref, get_ref_target, writes...
/// });
/// ```
#[macro_export]
macro_rules! forward_table_shell {
    ($ty:ty, $field:ident, { $($rest:tt)* }) => {
        #[$crate::__private::async_trait]
        impl $crate::TableShell for $ty {
            fn columns(&self) -> &$crate::__private::IndexMap<String, $crate::Column> {
                self.$field.columns()
            }
            fn references(&self) -> &$crate::__private::IndexMap<String, $crate::Reference> {
                self.$field.references()
            }
            fn contained(&self) -> &$crate::__private::IndexMap<String, $crate::ContainedSpec> {
                self.$field.contained()
            }
            fn id_column(&self) -> Option<&str> {
                self.$field.id_column()
            }
            fn driver_name(&self) -> &'static str {
                self.$field.driver_name()
            }
            fn preview_query(&self, vista: &$crate::Vista) -> $crate::__private::serde_json::Value {
                self.$field.preview_query(vista)
            }
            $crate::__forward_rhai_hooks!($field);

            async fn list_vista_values(
                &self,
                vista: &$crate::Vista,
            ) -> $crate::__private::Result<$crate::__private::IndexMap<String, $crate::__private::Rec>> {
                self.$field.list_vista_values(vista).await
            }
            async fn get_vista_value(
                &self,
                vista: &$crate::Vista,
                id: &String,
            ) -> $crate::__private::Result<Option<$crate::__private::Rec>> {
                self.$field.get_vista_value(vista, id).await
            }
            async fn get_vista_value_with_row(
                &self,
                vista: &$crate::Vista,
                id: &String,
                row: &$crate::__private::Rec,
            ) -> $crate::__private::Result<Option<$crate::__private::Rec>> {
                self.$field.get_vista_value_with_row(vista, id, row).await
            }
            async fn get_vista_some_value(
                &self,
                vista: &$crate::Vista,
            ) -> $crate::__private::Result<Option<(String, $crate::__private::Rec)>> {
                self.$field.get_vista_some_value(vista).await
            }
            async fn get_vista_count(&self, vista: &$crate::Vista) -> $crate::__private::Result<i64> {
                self.$field.get_vista_count(vista).await
            }
            async fn fetch_page(
                &self,
                vista: &$crate::Vista,
                page: usize,
            ) -> $crate::__private::Result<Vec<(String, $crate::__private::Rec)>> {
                self.$field.fetch_page(vista, page).await
            }
            async fn fetch_window(
                &self,
                vista: &$crate::Vista,
                offset: usize,
                limit: usize,
            ) -> $crate::__private::Result<Vec<(String, $crate::__private::Rec)>> {
                self.$field.fetch_window(vista, offset, limit).await
            }
            async fn fetch_window_counted(
                &self,
                vista: &$crate::Vista,
                offset: usize,
                limit: usize,
            ) -> $crate::__private::Result<(Vec<(String, $crate::__private::Rec)>, Option<i64>)> {
                self.$field.fetch_window_counted(vista, offset, limit).await
            }
            async fn watch_vista(
                &self,
                vista: &$crate::Vista,
            ) -> $crate::__private::Result<$crate::VistaChangeStream> {
                self.$field.watch_vista(vista).await
            }

            fn add_eq_condition(
                &mut self,
                field: &str,
                value: &$crate::CborValue,
            ) -> $crate::__private::Result<()> {
                self.$field.add_eq_condition(field, value)
            }
            fn add_op_condition(
                &mut self,
                field: &str,
                op: $crate::FilterOp,
                value: &$crate::CborValue,
            ) -> $crate::__private::Result<()> {
                self.$field.add_op_condition(field, op, value)
            }
            fn add_raw_condition(
                &mut self,
                condition: Box<dyn std::any::Any + Send + Sync>,
            ) -> $crate::__private::Result<()> {
                self.$field.add_raw_condition(condition)
            }
            fn set_page_size(&mut self, size: usize) -> $crate::__private::Result<()> {
                self.$field.set_page_size(size)
            }
            fn add_search(&mut self, text: &str) -> $crate::__private::Result<()> {
                self.$field.add_search(text)
            }
            fn clear_search(&mut self) -> $crate::__private::Result<()> {
                self.$field.clear_search()
            }
            fn add_order(
                &mut self,
                field: &str,
                dir: $crate::SortDirection,
            ) -> $crate::__private::Result<()> {
                self.$field.add_order(field, dir)
            }
            fn clear_orders(&mut self) -> $crate::__private::Result<()> {
                self.$field.clear_orders()
            }
            fn get_contained_ref(
                &self,
                relation: &str,
                row: &$crate::__private::Rec,
            ) -> $crate::__private::Result<$crate::Vista> {
                self.$field.get_contained_ref(relation, row)
            }

            $($rest)*
        }
    };
}

/// The Rhai hooks of [`forward_table_shell!`](crate::forward_table_shell);
/// empty without this crate's `rhai` feature, since the trait only has them
/// with it.
#[cfg(feature = "rhai")]
#[doc(hidden)]
#[macro_export]
macro_rules! __forward_rhai_hooks {
    ($field:ident) => {
        fn register_rhai_extensions(
            &self,
            engine: &mut $crate::__private::vantage_rhai::rhai::Engine,
        ) {
            self.$field.register_rhai_extensions(engine)
        }
        fn rhai_env(
            &self,
            env: $crate::__private::vantage_rhai::Env,
        ) -> $crate::__private::vantage_rhai::Env {
            self.$field.rhai_env(env)
        }
    };
}

#[cfg(not(feature = "rhai"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __forward_rhai_hooks {
    ($field:ident) => {};
}
