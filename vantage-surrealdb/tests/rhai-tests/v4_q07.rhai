// v4 Q7: time bucketing
// Tier 1: surreal::count(), avg, round. Existing: sum. Tier 2: time_group. No surreal::expr() stubs.
surreal::select()
    .from("order")
    .expression(surreal::time_group(surreal::ident("created_at"), "month").alias("month"))
    .expression(surreal::count().alias("orders"))
    .expression(surreal::sum(surreal::ident("total")).alias("revenue"))
    .expression(surreal::round(surreal::avg(surreal::ident("total"))).alias("avg_order_value"))
    .group_by(surreal::ident("month"))
    .order_by(surreal::ident("month"), "asc")
