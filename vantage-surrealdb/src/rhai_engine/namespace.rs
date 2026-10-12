//! The `surreal::` namespace: every free function of the SurrealDB
//! vocabulary, and `surreal::me`, the current-record anchor.

use vantage_rhai::Namespace;
use vantage_rhai::rhai::Engine;

use super::constructors as c;
use super::select_methods::select_new;
use super::types::RhaiExpr;

pub(super) fn register_namespace(engine: &mut Engine) {
    let mut ns = Namespace::new("surreal");
    ns.var("me", RhaiExpr(crate::primitives::me()));

    // Constructors
    ns.func("ident", c::make_rhai_ident)
        .func("expr", c::make_expr0)
        .func("expr", c::make_expr)
        .func("fx", c::make_fx)
        .func("select", select_new)
        .func("case_when", c::fn_case_new);

    // Aggregates
    ns.func("count", c::fn_count_expr)
        .func("count", c::fn_count_of)
        .func("count_distinct", c::fn_count_distinct)
        .func("sum", c::fn_sum)
        .func("avg", c::fn_avg)
        .func("min", c::fn_min)
        .func("max", c::fn_max);

    // Scalar and conditional functions shared with SQL
    ns.func("round", c::fn_round)
        .func("round", c::fn_round_to)
        .func("coalesce", c::fn_coalesce)
        .func("nullif", c::fn_nullif)
        .func("cast", c::fn_cast)
        .func("date_format", c::fn_date_format);

    // SurrealDB scalar and collection functions
    ns.func("first", c::fn_first)
        .func("len", c::fn_len)
        .func("stddev", c::fn_stddev)
        .func("median", c::fn_median)
        .func("lower", c::fn_lower)
        .func("words", c::fn_words)
        .func("object_entries", c::fn_object_entries)
        .func("object_values", c::fn_object_values)
        .func("time_group", c::fn_time_group)
        .func("similarity", c::fn_similarity)
        .func("array_group", c::fn_array_group)
        .func("time_now", c::fn_time_now)
        .func("string_len", c::fn_string_len)
        .func("type_float", c::fn_type_float)
        .func("type_int", c::fn_type_int);

    // Arithmetic
    ns.func("mul", c::fn_mul)
        .func("add", c::fn_add)
        .func("sub", c::fn_sub)
        .func("div", c::fn_div);

    // Records, parameters and graph paths
    ns.func("thing", c::fn_thing)
        .func("param", c::fn_param)
        .func("parent", c::fn_parent)
        .func("parent", c::fn_parent_bare)
        .func("graph", c::fn_graph2)
        .func("graph", c::fn_graph3)
        .func("graph", c::fn_graph4)
        .func("graph", c::fn_graph5)
        .func("graph", c::fn_graph6)
        .func("graph", c::fn_graph7)
        .func("recurse", c::fn_recurse);

    ns.register(engine);
}
