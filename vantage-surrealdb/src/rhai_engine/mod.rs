//! Rhai scripting engine for building SurrealDB queries.
//!
//! Self-contained module — does not depend on vantage-sql's rhai_engine.
//! Registers wrapper types, constructors, operators, and select builder
//! methods into a Rhai engine.

use vantage_rhai::rhai;

#[cfg(feature = "rhai")]
pub mod constructors;
#[cfg(feature = "rhai")]
pub mod convert;
#[cfg(feature = "rhai")]
mod namespace;
#[cfg(feature = "rhai")]
pub mod operators;
#[cfg(feature = "rhai")]
pub mod select_methods;
#[cfg(feature = "rhai")]
pub mod types;

#[cfg(feature = "rhai")]
pub use types::{RhaiCase, RhaiExpr, RhaiIdent, RhaiSelect};

/// The SurrealDB query-building vocabulary as a [`vantage_rhai::Vocab`].
#[cfg(feature = "rhai")]
pub struct SurrealVocab;

#[cfg(feature = "rhai")]
impl vantage_rhai::Vocab for SurrealVocab {
    fn register(&self, engine: &mut rhai::Engine) {
        register_surreal_onto(engine);
    }
}

/// Register the full SurrealDB query-building vocabulary onto `engine`.
///
/// The free functions (constructors such as `ident`, `expr`, `select`,
/// `thing` and `graph`, the aggregates, the scalar functions) live in the
/// `surreal::` namespace, and so does `surreal::me`, the current-record
/// anchor: `surreal::graph(surreal::me, "placed", "order")`. Methods and
/// operators on the vocabulary's own types (`.from`, `.where`, `.alias`,
/// `["col"]`, `==`, `*`, ..) stay global, since they dispatch on their
/// receiver and can't shadow the data vocabulary's verbs.
///
/// Shared by two entry points:
/// - [`SurrealVocab`], the form a [`vantage_rhai::Host`] takes (the `rhai:`
///   vista source, the standalone Rhai tests/examples via
///   [`register_surreal_engine!`](crate::register_surreal_engine));
/// - `SurrealTableShell::register_rhai_extensions`, which layers it on top of
///   vantage-vista's conventional `Vista` verbs so a reference build-script can
///   use `ident`/`==`/`fx`/graph syntax and `with_condition`.
///
/// Prefer [`SurrealVocab`] on a host — this is the registration behind it.
/// Limits are the host's business, not this function's.
#[cfg(feature = "rhai")]
pub fn register_surreal_onto(engine: &mut rhai::Engine) {
    use crate::AnySurrealType as AST;
    use crate::rhai_engine::types::{RhaiExpr as Ex, RhaiIdent as Id};

    namespace::register_namespace(engine);

    {
        // ── Register types ────────────────────────────────────
        engine.register_type::<crate::rhai_engine::types::RhaiSelect>();
        engine.register_type::<Id>();
        engine.register_type::<Ex>();
        engine.register_type::<crate::rhai_engine::types::RhaiCase>();

        // ── surreal::case_when().when().else_().expr() → IF … END ──
        engine.register_fn("when", crate::rhai_engine::constructors::fn_case_when);
        engine.register_fn("else_", crate::rhai_engine::constructors::fn_case_else);
        engine.register_fn("expr", crate::rhai_engine::constructors::fn_case_expr);
        engine.register_fn(
            "clone",
            |c: crate::rhai_engine::types::RhaiCase| -> crate::rhai_engine::types::RhaiCase { c },
        );

        // ── Ident methods ─────────────────────────────────────
        engine.register_fn("dot_of", |id: &mut Id, field: &str| -> Id {
            crate::rhai_engine::constructors::ident_dot(id.0.clone(), field)
        });
        // Aliasing a name/path projects it: `dept.name AS department`. Lifts the
        // ident into the expression layer (the alias lives there), so it composes
        // like any other `.alias()` on an Ex.
        engine.register_fn("alias", |id: &mut Id, alias: &str| -> Ex {
            Ex(crate::rhai_engine::constructors::ident_as_alias(
                id.0.clone(),
                alias,
            ))
        });

        // ── Expr methods ──────────────────────────────────────
        engine.register_fn("alias", |e: &mut Ex, alias: &str| -> Ex {
            crate::rhai_engine::types::RhaiExpr(crate::rhai_engine::constructors::expr_as_alias(
                e.0.clone(),
                alias,
            ))
        });

        // ── Indexer: t["col"] / expr["field"] ─────────────────
        engine.register_indexer_get(|id: &mut Id, col: &str| -> Ex {
            crate::rhai_engine::constructors::ident_index(id.0.clone(), col)
        });
        engine.register_indexer_get(|e: &mut Ex, col: &str| -> Ex {
            Ex(crate::rhai_engine::constructors::expr_index(
                e.0.clone(),
                col,
            ))
        });
        engine.register_indexer_get(|e: &mut Ex, n: i64| -> Ex {
            Ex(crate::rhai_engine::constructors::expr_index_at(
                e.0.clone(),
                n,
            ))
        });

        // ── Comparison operators ──────────────────────────────
        macro_rules! reg_op {
            ($rhai_op:expr, $surreal_op:expr) => {
                engine.register_fn(
                    $rhai_op,
                    |a: rhai::Dynamic, b: rhai::Dynamic| -> Result<Ex, Box<rhai::EvalAltResult>> {
                        let la = crate::rhai_engine::operators::to_expr(a)?;
                        let rb = crate::rhai_engine::operators::to_expr(b)?;
                        Ok(crate::rhai_engine::operators::cmp_binary(
                            $surreal_op,
                            la,
                            rb,
                        ))
                    },
                );
            };
        }
        reg_op!("==", "=");
        reg_op!("!=", "!=");
        reg_op!("<", "<");
        reg_op!(">", ">");
        reg_op!("<=", "<=");
        reg_op!(">=", ">=");

        // ── Arithmetic operators on Ex (closure bodies, ad-hoc maths) ──
        // Only combos involving an Ex are registered, so native numeric
        // arithmetic (i64*i64, …) is untouched. Each renders parenthesized.
        macro_rules! reg_arith {
            ($op:expr) => {{
                engine.register_fn($op, |a: Ex, b: Ex| -> Ex {
                    crate::rhai_engine::operators::arith($op, a.0, b.0)
                });
                engine.register_fn($op, |a: Ex, b: i64| -> Ex {
                    crate::rhai_engine::operators::arith(
                        $op,
                        a.0,
                        crate::rhai_engine::operators::scalar(AST::from(b)),
                    )
                });
                engine.register_fn($op, |a: i64, b: Ex| -> Ex {
                    crate::rhai_engine::operators::arith(
                        $op,
                        crate::rhai_engine::operators::scalar(AST::from(a)),
                        b.0,
                    )
                });
                engine.register_fn($op, |a: Ex, b: f64| -> Ex {
                    crate::rhai_engine::operators::arith(
                        $op,
                        a.0,
                        crate::rhai_engine::operators::scalar(AST::from(b)),
                    )
                });
                engine.register_fn($op, |a: f64, b: Ex| -> Ex {
                    crate::rhai_engine::operators::arith(
                        $op,
                        crate::rhai_engine::operators::scalar(AST::from(a)),
                        b.0,
                    )
                });
            }};
        }
        reg_arith!("*");
        reg_arith!("+");
        reg_arith!("-");
        reg_arith!("/");

        // ── Select methods ────────────────────────────────────
        engine.register_fn("from", crate::rhai_engine::select_methods::select_from);
        engine.register_fn("from", crate::rhai_engine::select_methods::select_from_id);
        engine.register_fn("from", crate::rhai_engine::select_methods::select_from_expr);
        engine.register_fn("field", crate::rhai_engine::select_methods::select_field);
        engine.register_fn(
            "clear_fields",
            crate::rhai_engine::select_methods::select_clear_fields,
        );
        engine.register_fn(
            "expression",
            crate::rhai_engine::select_methods::select_expression,
        );
        engine.register_fn(
            "expression",
            crate::rhai_engine::select_methods::select_expression_id,
        );
        engine.register_fn("where", crate::rhai_engine::select_methods::select_where);
        engine.register_fn(
            "group_by",
            crate::rhai_engine::select_methods::select_group_by,
        );
        engine.register_fn(
            "group_by",
            crate::rhai_engine::select_methods::select_group_by_id,
        );
        engine.register_fn(
            "order_by",
            crate::rhai_engine::select_methods::select_order_by,
        );
        engine.register_fn(
            "order_by",
            crate::rhai_engine::select_methods::select_order_by_id,
        );
        engine.register_fn(
            "distinct",
            crate::rhai_engine::select_methods::select_distinct,
        );
        engine.register_fn(
            "group_all",
            crate::rhai_engine::select_methods::select_group_all,
        );
        engine.register_fn("split", crate::rhai_engine::select_methods::select_split);
        engine.register_fn("split", crate::rhai_engine::select_methods::select_split_id);
        engine.register_fn("limit", crate::rhai_engine::select_methods::select_limit);

        // ── SurrealDB-specific select methods ─────────────────
        engine.register_fn("only", crate::rhai_engine::select_methods::select_only);
        engine.register_fn("value", crate::rhai_engine::select_methods::select_value);
        engine.register_fn(
            "subquery",
            crate::rhai_engine::select_methods::select_subquery,
        );

        // ── Graph traversal ───────────────────────────────────
        engine.register_fn("arrow", crate::rhai_engine::select_methods::graph_arrow);
        engine.register_fn("back", crate::rhai_engine::select_methods::graph_back);
        engine.register_fn(
            "arrow_field",
            crate::rhai_engine::select_methods::graph_arrow_field,
        );

        // ── Embedded-array closures: .map / .fold / .filter ───────────
        engine.register_fn("map", crate::rhai_engine::constructors::fn_map);
        engine.register_fn("fold", crate::rhai_engine::constructors::fn_fold);
        engine.register_fn("filter", crate::rhai_engine::constructors::fn_filter);

        // ── Clone ─────────────────────────────────────────────
        engine.register_fn("clone", |e: Ex| -> Ex { e });
        engine.register_fn("clone", |id: Id| -> Id { id });
    }
}

/// Bring the SurrealDB Rhai toolkit into scope: the `AST`/`Ex`/`Id`/`Sel`
/// aliases and `__host()`, one shared [`vantage_rhai::Host`] carrying
/// [`SurrealVocab`] under the background limit profile. Evaluate with
/// `__host().compile(..)`; `surreal::me` needs no `Env` setup.
#[cfg(feature = "rhai")]
#[macro_export]
macro_rules! register_surreal_engine {
    () => {
        // Back-compat: callers still reference these aliases (e.g. `Sel`) after
        // invoking the macro, so keep them in scope.
        use $crate::AnySurrealType as AST;
        use $crate::rhai_engine::types::{RhaiExpr as Ex, RhaiIdent as Id, RhaiSelect as Sel};

        fn __host() -> &'static vantage_rhai::Host {
            static HOST: std::sync::LazyLock<vantage_rhai::Host> = std::sync::LazyLock::new(|| {
                vantage_rhai::Host::builder(vantage_rhai::Limits::background())
                    .vocab($crate::rhai_engine::SurrealVocab)
                    .build()
            });
            &HOST
        }
    };
}
