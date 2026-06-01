//! Rhai scripting engine for building SQL queries.
//!
//! Each SQL backend invokes `register_engine!` with its own types.
//! Sub-macros (`register_types!`, `register_convert!`, `register_constructors!`,
//! etc.) each generate a piece of the engine. They share type aliases
//! defined once by `register_engine!` in the same expansion scope.

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

// Future checkpoint modules — uncomment as implemented:
// #[macro_use] pub mod aggregates;
// #[macro_use] pub mod window_case;

// Re-export wrapper types so macros can reference them via $crate
pub use types::{RhaiCase, RhaiExpr, RhaiIdent, RhaiSelect, RhaiWindow};

#[macro_export]
macro_rules! register_engine {
    (
        value: $V:ty,
        select: $Select:ty,
        join: $Join:ty,
        cond: $Cond:ty
        $(,)?
    ) => {
        // ── Shared type aliases ─────────────────────────────────────
        type Expr = $crate::vantage_expressions::Expression<$V>;
        type Sel = $crate::rhai_engine::RhaiSelect<$V, $Select, $Join, $Cond>;
        type Id = $crate::rhai_engine::RhaiIdent;
        type Ex = $crate::rhai_engine::RhaiExpr<$V>;
        type Win = $crate::rhai_engine::RhaiWindow<$V>;
        type Cas = $crate::rhai_engine::RhaiCase<$V>;

        fn __create_engine() -> rhai::Engine {
            let mut engine = rhai::Engine::new();

            $crate::register_types!(engine, value: $V, select: $Select, join: $Join, cond: $Cond);
            $crate::register_convert!(value: $V);
            $crate::register_constructors!(engine, value: $V);

            $crate::register_select!(engine, value: $V, select: $Select, join: $Join, cond: $Cond);
            $crate::register_operators!(engine, value: $V);
            $crate::register_window_case!(engine, value: $V);

            // Future phases:
            // $crate::register_aggregates!(engine, value: $V);
            // $crate::register_window_case!(engine, value: $V);

            engine.set_max_expr_depths(256, 256);
            engine
        }
    };
}
