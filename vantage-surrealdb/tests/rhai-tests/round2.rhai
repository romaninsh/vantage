// 2-arg round → math::fixed(expr, places): round to N decimal places
// (SurrealDB's math::round is integer-only).
surreal::select()
    .from("product")
    .expression(surreal::round(surreal::avg(surreal::ident("price")), 2).alias("avg_price"))
