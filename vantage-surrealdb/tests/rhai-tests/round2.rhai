// 2-arg round → math::fixed(expr, places): round to N decimal places
// (SurrealDB's math::round is integer-only).
select()
    .from("product")
    .expression(round(avg(ident("price")), 2).alias("avg_price"))
