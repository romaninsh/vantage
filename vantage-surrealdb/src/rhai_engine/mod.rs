//! Rhai scripting engine for building SurrealDB queries.
//!
//! Self-contained module — does not depend on vantage-sql's rhai_engine.
//! Registers wrapper types, constructors, operators, and select builder
//! methods into a Rhai engine.

#[cfg(feature = "rhai")]
pub mod constructors;
#[cfg(feature = "rhai")]
pub mod convert;
#[cfg(feature = "rhai")]
pub mod operators;
#[cfg(feature = "rhai")]
pub mod select_methods;
#[cfg(feature = "rhai")]
pub mod types;

#[cfg(feature = "rhai")]
pub use types::{RhaiCase, RhaiExpr, RhaiIdent, RhaiSelect};

/// Register the full SurrealDB query-building vocabulary onto `engine`.
///
/// Shared by two entry points:
/// - the [`register_surreal_engine!`] macro, whose `__create_engine` wraps this
///   in a fresh `Engine` (back-compat for the standalone Rhai tests/examples and
///   the `rhai:` vista source);
/// - `SurrealTableShell::register_rhai_extensions`, which layers it on top of
///   vantage-vista's conventional `Vista` verbs so a reference build-script can
///   use `ident`/`==`/`fx`/graph syntax and `with_condition`.
#[cfg(feature = "rhai")]
pub fn register_surreal_onto(engine: &mut rhai::Engine) {
    use crate::AnySurrealType as AST;
    use crate::rhai_engine::types::{RhaiExpr as Ex, RhaiIdent as Id};

    {
        // ── Register types ────────────────────────────────────
        engine.register_type::<crate::rhai_engine::types::RhaiSelect>();
            engine.register_type::<Id>();
            engine.register_type::<Ex>();
            engine.register_type::<crate::rhai_engine::types::RhaiCase>();

            // ── Constructors ──────────────────────────────────────
            engine.register_fn("ident", crate::rhai_engine::constructors::make_rhai_ident);
            engine.register_fn("table", crate::rhai_engine::constructors::make_rhai_ident);
            engine.register_fn("expr", crate::rhai_engine::constructors::make_expr0);
            engine.register_fn("expr", crate::rhai_engine::constructors::make_expr);
            engine.register_fn("fx", crate::rhai_engine::constructors::make_fx);

            // ── Aggregates ────────────────────────────────────────
            engine.register_fn("count", crate::rhai_engine::constructors::fn_count_expr);
            engine.register_fn("count", crate::rhai_engine::constructors::fn_count_of);
            engine.register_fn(
                "count_distinct",
                crate::rhai_engine::constructors::fn_count_distinct,
            );
            engine.register_fn("sum", crate::rhai_engine::constructors::fn_sum);
            engine.register_fn("avg", crate::rhai_engine::constructors::fn_avg);
            engine.register_fn("min", crate::rhai_engine::constructors::fn_min);
            engine.register_fn("max", crate::rhai_engine::constructors::fn_max);

            // ── Tier 1 shared scalar/conditional primitives ───────
            engine.register_fn("round", crate::rhai_engine::constructors::fn_round);
            engine.register_fn("round", crate::rhai_engine::constructors::fn_round_to);
            engine.register_fn("coalesce", crate::rhai_engine::constructors::fn_coalesce);
            engine.register_fn("nullif", crate::rhai_engine::constructors::fn_nullif);
            engine.register_fn("cast", crate::rhai_engine::constructors::fn_cast);
            engine.register_fn(
                "date_format",
                crate::rhai_engine::constructors::fn_date_format,
            );

            // ── Tier 2 surreal-specific scalar/collection functions ───
            engine.register_fn("first", crate::rhai_engine::constructors::fn_first);
            engine.register_fn("len", crate::rhai_engine::constructors::fn_len);
            engine.register_fn("stddev", crate::rhai_engine::constructors::fn_stddev);
            engine.register_fn("median", crate::rhai_engine::constructors::fn_median);
            engine.register_fn("lower", crate::rhai_engine::constructors::fn_lower);
            engine.register_fn("words", crate::rhai_engine::constructors::fn_words);
            engine.register_fn(
                "object_entries",
                crate::rhai_engine::constructors::fn_object_entries,
            );
            engine.register_fn(
                "object_values",
                crate::rhai_engine::constructors::fn_object_values,
            );
            engine.register_fn(
                "time_group",
                crate::rhai_engine::constructors::fn_time_group,
            );
            engine.register_fn(
                "similarity",
                crate::rhai_engine::constructors::fn_similarity,
            );

            // ── case_when().when().else_().expr() → IF … END ──────
            engine.register_fn("case_when", crate::rhai_engine::constructors::fn_case_new);
            engine.register_fn("when", crate::rhai_engine::constructors::fn_case_when);
            engine.register_fn("else_", crate::rhai_engine::constructors::fn_case_else);
            engine.register_fn("expr", crate::rhai_engine::constructors::fn_case_expr);
            engine.register_fn(
                "clone",
                |c: crate::rhai_engine::types::RhaiCase| -> crate::rhai_engine::types::RhaiCase {
                    c
                },
            );

            // ── Arithmetic ────────────────────────────────────────
            engine.register_fn("mul", crate::rhai_engine::constructors::fn_mul);
            engine.register_fn("add", crate::rhai_engine::constructors::fn_add);
            engine.register_fn("sub", crate::rhai_engine::constructors::fn_sub);
            engine.register_fn("div", crate::rhai_engine::constructors::fn_div);

            // ── SurrealDB-specific constructors ───────────────────
            engine.register_fn("thing", crate::rhai_engine::constructors::fn_thing);
            engine.register_fn("param", crate::rhai_engine::constructors::fn_param);
            engine.register_fn("parent", crate::rhai_engine::constructors::fn_parent);
            engine.register_fn("parent", crate::rhai_engine::constructors::fn_parent_bare);
            engine.register_fn(
                "array_group",
                crate::rhai_engine::constructors::fn_array_group,
            );
            engine.register_fn("time_now", crate::rhai_engine::constructors::fn_time_now);
            engine.register_fn(
                "string_len",
                crate::rhai_engine::constructors::fn_string_len,
            );
            engine.register_fn(
                "type_float",
                crate::rhai_engine::constructors::fn_type_float,
            );
            engine.register_fn("type_int", crate::rhai_engine::constructors::fn_type_int);

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
                crate::rhai_engine::types::RhaiExpr(
                    crate::rhai_engine::constructors::expr_as_alias(e.0.clone(), alias),
                )
            });

            // ── Indexer: t["col"] / expr["field"] ─────────────────
            engine.register_indexer_get(|id: &mut Id, col: &str| -> Id {
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
                        |a: rhai::Dynamic,
                         b: rhai::Dynamic|
                         -> Result<Ex, Box<rhai::EvalAltResult>> {
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

            // ── Select constructor ────────────────────────────────
            engine.register_fn("select", crate::rhai_engine::select_methods::select_new);

            // ── Select methods ────────────────────────────────────
            engine.register_fn("from", crate::rhai_engine::select_methods::select_from);
            engine.register_fn("from", crate::rhai_engine::select_methods::select_from_id);
            engine.register_fn(
                "from",
                crate::rhai_engine::select_methods::select_from_expr,
            );
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
            engine.register_fn(
                "split",
                crate::rhai_engine::select_methods::select_split_id,
            );
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

            // graph(me, "edge", "table", …) — direction set by `me`'s position
            engine.register_fn("graph", crate::rhai_engine::constructors::fn_graph2);
            engine.register_fn("graph", crate::rhai_engine::constructors::fn_graph3);
            engine.register_fn("graph", crate::rhai_engine::constructors::fn_graph4);
            engine.register_fn("graph", crate::rhai_engine::constructors::fn_graph5);
            engine.register_fn("graph", crate::rhai_engine::constructors::fn_graph6);
            engine.register_fn("graph", crate::rhai_engine::constructors::fn_graph7);
            engine.register_fn("recurse", crate::rhai_engine::constructors::fn_recurse);

            // ── Embedded-array closures: .map / .fold / .filter ───────────
            engine.register_fn("map", crate::rhai_engine::constructors::fn_map);
            engine.register_fn("fold", crate::rhai_engine::constructors::fn_fold);
            engine.register_fn("filter", crate::rhai_engine::constructors::fn_filter);

            // `me` — the current-record anchor, available as a bare constant.
            // `on_var` is marked volatile (not deprecated) by rhai; silence the lint.
            #[allow(deprecated)]
            {
                engine.on_var(|name, _index, _context| {
                    if name == "me" {
                        Ok(Some(rhai::Dynamic::from(Ex(crate::primitives::me()))))
                    } else {
                        Ok(None)
                    }
                });
            }

            // ── Clone ─────────────────────────────────────────────
            engine.register_fn("clone", |e: Ex| -> Ex { e });
            engine.register_fn("clone", |id: Id| -> Id { id });

        engine.set_max_expr_depths(256, 256);
    }
}

#[cfg(feature = "rhai")]
#[macro_export]
macro_rules! register_surreal_engine {
    () => {
        // Back-compat: callers still reference these aliases (e.g. `Sel`) after
        // invoking the macro, so keep them in scope.
        use $crate::AnySurrealType as AST;
        use $crate::rhai_engine::types::{RhaiExpr as Ex, RhaiIdent as Id, RhaiSelect as Sel};

        fn __create_engine() -> rhai::Engine {
            let mut engine = rhai::Engine::new();
            $crate::rhai_engine::register_surreal_onto(&mut engine);
            engine
        }
    };
}
