//! Rhai scripting engine for building SQL queries.
//!
//! Each SQL backend invokes `register_engine!` with its own types.
//! Sub-macros (`register_types!`, `register_convert!`, `register_constructors!`,
//! etc.) each generate a piece of the engine. They share type aliases
//! defined once by `register_engine!` in the same expansion scope.
//!
//! A script calls the free functions through the `sql::` namespace
//! (`sql::select().from(sql::ident("users"))`); the methods and operators on
//! the SQL types (`.from`, `.where`, `.alias`, `["col"]`, `==`) are global.

// Public so `register_engine!` can name the host crate through `$crate` from
// any invocation site (a test, an example) without that site depending on it.
pub use vantage_rhai;
pub use vantage_rhai::rhai;

#[macro_use]
pub mod types;
#[macro_use]
pub mod convert;
#[macro_use]
pub mod constructors;

#[macro_use]
pub mod select_methods;

#[macro_use]
pub mod operators;
pub mod window_case;

// Re-export wrapper types so macros can reference them via $crate
pub use types::{RhaiCase, RhaiExpr, RhaiIdent, RhaiSelect, RhaiWindow};

/// Generate one SQL dialect's Rhai vocabulary in the invoking scope: the
/// `SqlVocab` vocabulary and a shared `__host()`. The free functions are
/// registered under `sql::`, one namespace for every dialect, so an engine
/// keeps one `sql::` dialect: the last one registered on it.
#[macro_export]
macro_rules! register_engine {
    (
        value: $V:ty,
        select: $Select:ty,
        join: $Join:ty,
        cond: $Cond:ty
        $(,)?
    ) => {
        // The sub-macros below name `rhai::` bare, so the expansion brings the
        // host crate's rhai into the invoking scope. Callers must not import
        // it themselves.
        #[allow(unused_imports)]
        use $crate::rhai_engine::rhai;

        // ── Shared type aliases ─────────────────────────────────────
        type Expr = $crate::vantage_expressions::Expression<$V>;
        type Sel = $crate::rhai_engine::RhaiSelect<$V, $Select, $Join, $Cond>;
        type Id = $crate::rhai_engine::RhaiIdent;
        type Ex = $crate::rhai_engine::RhaiExpr<$V>;
        type Win = $crate::rhai_engine::RhaiWindow<$V>;
        type Cas = $crate::rhai_engine::RhaiCase<$V>;

        // Register the full SQL vocabulary onto an existing engine, so a host
        // can combine it with other vocabularies (e.g. vantage-vista's data
        // vocabulary for `modify:` scripts and scripted references) instead
        // of using `SqlVocab` on its own. The free functions go into the
        // `sql::` namespace, the same for every dialect, so a script stays
        // portable; methods and operators on the SQL types stay global.
        fn __register_engine_onto(engine: &mut $crate::rhai_engine::rhai::Engine) {
            let mut ns = $crate::rhai_engine::vantage_rhai::Namespace::new("sql");
            $crate::register_types!(engine, value: $V, select: $Select, join: $Join, cond: $Cond);
            $crate::register_convert!(value: $V);
            $crate::register_constructors!(engine, ns, value: $V);

            $crate::register_select!(engine, ns, value: $V, select: $Select, join: $Join, cond: $Cond);
            $crate::register_operators!(engine, value: $V);
            $crate::register_window_case!(engine, ns, value: $V);
            ns.register(engine);
        }

        /// This dialect's SQL vocabulary as a [`Vocab`](vantage_rhai::Vocab).
        pub struct SqlVocab;

        impl $crate::rhai_engine::vantage_rhai::Vocab for SqlVocab {
            fn register(&self, engine: &mut $crate::rhai_engine::rhai::Engine) {
                __register_engine_onto(engine);
            }
        }

        /// One shared host per dialect: background limits (query scripts run
        /// while a table builds, never on a UI thread), `SqlVocab`, built once.
        /// Query-source scripts repeat per table, so compile through its cache.
        fn __host() -> &'static $crate::rhai_engine::vantage_rhai::Host {
            static HOST: std::sync::LazyLock<$crate::rhai_engine::vantage_rhai::Host> =
                std::sync::LazyLock::new(|| {
                    $crate::rhai_engine::vantage_rhai::Host::builder(
                        $crate::rhai_engine::vantage_rhai::Limits::background(),
                    )
                    .vocab(SqlVocab)
                    .build()
                });
            &HOST
        }
    };
}
