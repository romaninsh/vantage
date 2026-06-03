// v4 Q7: time bucketing
// Tier 1: count(), avg, round. Existing: sum. Tier 2: time_group. No expr() stubs.
select()
    .from("order")
    .expression(time_group(ident("created_at"), "month").alias("month"))
    .expression(count().alias("orders"))
    .expression(sum(ident("total")).alias("revenue"))
    .expression(round(avg(ident("total"))).alias("avg_order_value"))
    .group_by(ident("month"))
    .order_by(ident("month"), "asc")
