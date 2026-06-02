//! Rhai scripting engine for building SurrealDB queries.
//!
//! Self-contained module — does not depend on vantage-sql's rhai_engine.
//! Registers wrapper types, constructors, operators, and select builder
//! methods into a Rhai engine.

#[cfg(feature = "rhai")]
pub mod types;
#[cfg(feature = "rhai")]
pub mod convert;
#[cfg(feature = "rhai")]
pub mod constructors;
#[cfg(feature = "rhai")]
pub mod operators;
#[cfg(feature = "rhai")]
pub mod select_methods;

#[cfg(feature = "rhai")]
pub use types::{RhaiExpr, RhaiIdent, RhaiSelect};

#[cfg(feature = "rhai")]
#[macro_export]
macro_rules! register_surreal_engine {
    () => {
        use $crate::rhai_engine::types::{RhaiExpr as Ex, RhaiIdent as Id, RhaiSelect as Sel};
        use $crate::AnySurrealType as AST;

        fn __create_engine() -> rhai::Engine {
            let mut engine = rhai::Engine::new();

            // ── Register types ────────────────────────────────────
            engine.register_type::<Sel>();
            engine.register_type::<Id>();
            engine.register_type::<Ex>();

            // ── Constructors ──────────────────────────────────────
            engine.register_fn("ident", $crate::rhai_engine::constructors::make_rhai_ident);
            engine.register_fn("table", $crate::rhai_engine::constructors::make_rhai_ident);
            engine.register_fn("expr", $crate::rhai_engine::constructors::make_expr0);
            engine.register_fn("expr", $crate::rhai_engine::constructors::make_expr);
            engine.register_fn("fx", $crate::rhai_engine::constructors::make_fx);

            // ── Aggregates ────────────────────────────────────────
            engine.register_fn("count", $crate::rhai_engine::constructors::fn_count_expr);
            engine.register_fn("sum", $crate::rhai_engine::constructors::fn_sum);
            engine.register_fn("min", $crate::rhai_engine::constructors::fn_min);
            engine.register_fn("max", $crate::rhai_engine::constructors::fn_max);

            // ── Arithmetic ────────────────────────────────────────
            engine.register_fn("mul", $crate::rhai_engine::constructors::fn_mul);
            engine.register_fn("add", $crate::rhai_engine::constructors::fn_add);
            engine.register_fn("sub", $crate::rhai_engine::constructors::fn_sub);
            engine.register_fn("div", $crate::rhai_engine::constructors::fn_div);

            // ── SurrealDB-specific constructors ───────────────────
            engine.register_fn("thing", $crate::rhai_engine::constructors::fn_thing);
            engine.register_fn("parent", $crate::rhai_engine::constructors::fn_parent);
            engine.register_fn("parent", $crate::rhai_engine::constructors::fn_parent_bare);
            engine.register_fn("array_group", $crate::rhai_engine::constructors::fn_array_group);
            engine.register_fn("time_now", $crate::rhai_engine::constructors::fn_time_now);
            engine.register_fn("string_len", $crate::rhai_engine::constructors::fn_string_len);
            engine.register_fn("type_float", $crate::rhai_engine::constructors::fn_type_float);
            engine.register_fn("type_int", $crate::rhai_engine::constructors::fn_type_int);

            // ── Ident methods ─────────────────────────────────────
            engine.register_fn("dot_of", |id: &mut Id, field: &str| -> Id {
                $crate::rhai_engine::constructors::ident_dot(id.0.clone(), field)
            });
            engine.register_fn("alias", |id: &mut Id, alias: &str| -> Id {
                // SurrealDB aliases are expressed in the expression layer,
                // but for ident we just rename
                $crate::rhai_engine::types::RhaiIdent(
                    $crate::identifier::Identifier::new(alias)
                )
            });

            // ── Expr methods ──────────────────────────────────────
            engine.register_fn("alias", |e: &mut Ex, alias: &str| -> Ex {
                $crate::rhai_engine::types::RhaiExpr(
                    $crate::rhai_engine::constructors::expr_as_alias(e.0.clone(), alias)
                )
            });

            // ── Indexer: t["col"] ─────────────────────────────────
            engine.register_indexer_get(|id: &mut Id, col: &str| -> Id {
                $crate::rhai_engine::constructors::ident_index(id.0.clone(), col)
            });

            // ── Comparison operators ──────────────────────────────
            macro_rules! reg_op {
                ($rhai_op:expr, $surreal_op:expr) => {
                    engine.register_fn($rhai_op, |a: rhai::Dynamic, b: rhai::Dynamic|
                        -> Result<Ex, Box<rhai::EvalAltResult>>
                    {
                        let la = $crate::rhai_engine::operators::to_expr(a)?;
                        let rb = $crate::rhai_engine::operators::to_expr(b)?;
                        Ok($crate::rhai_engine::operators::cmp_binary($surreal_op, la, rb))
                    });
                };
            }
            reg_op!("==", "=");
            reg_op!("!=", "!=");
            reg_op!("<", "<");
            reg_op!(">", ">");
            reg_op!("<=", "<=");
            reg_op!(">=", ">=");

            // ── Select constructor ────────────────────────────────
            engine.register_fn("select", $crate::rhai_engine::select_methods::select_new);

            // ── Select methods ────────────────────────────────────
            engine.register_fn("from", $crate::rhai_engine::select_methods::select_from);
            engine.register_fn("from", $crate::rhai_engine::select_methods::select_from_id);
            engine.register_fn("from", $crate::rhai_engine::select_methods::select_from_expr);
            engine.register_fn("field", $crate::rhai_engine::select_methods::select_field);
            engine.register_fn("expression", $crate::rhai_engine::select_methods::select_expression);
            engine.register_fn("expression", $crate::rhai_engine::select_methods::select_expression_id);
            engine.register_fn("where", $crate::rhai_engine::select_methods::select_where);
            engine.register_fn("group_by", $crate::rhai_engine::select_methods::select_group_by);
            engine.register_fn("group_by", $crate::rhai_engine::select_methods::select_group_by_id);
            engine.register_fn("order_by", $crate::rhai_engine::select_methods::select_order_by);
            engine.register_fn("order_by", $crate::rhai_engine::select_methods::select_order_by_id);
            engine.register_fn("distinct", $crate::rhai_engine::select_methods::select_distinct);
            engine.register_fn("limit", $crate::rhai_engine::select_methods::select_limit);

            // ── SurrealDB-specific select methods ─────────────────
            engine.register_fn("only", $crate::rhai_engine::select_methods::select_only);
            engine.register_fn("value", $crate::rhai_engine::select_methods::select_value);

            // ── Graph traversal ───────────────────────────────────
            engine.register_fn("arrow", $crate::rhai_engine::select_methods::graph_arrow);
            engine.register_fn("back", $crate::rhai_engine::select_methods::graph_back);
            engine.register_fn("arrow_field", $crate::rhai_engine::select_methods::graph_arrow_field);

            // ── Clone ─────────────────────────────────────────────
            engine.register_fn("clone", |e: Ex| -> Ex { e });
            engine.register_fn("clone", |id: Id| -> Id { id });

            engine.set_max_expr_depths(256, 256);
            engine
        }
    };
}
