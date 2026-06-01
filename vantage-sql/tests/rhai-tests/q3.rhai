// Q3: Monthly Revenue Trend
// Features: date_format for time buckets, COUNT(DISTINCT), GROUP BY derived column

let o = table("client_order").alias("o");
let ol = table("order_line").alias("ol");

select()
    .from(o)
    .expression(date_format(o["created_at"], "%Y-%m").alias("month"))
    .expression(count_distinct(o["id"]).alias("orders"))
    .expression(coalesce(sum(mul(ol["quantity"], ol["price"])), expr("0")).alias("revenue"))
    .left_join("order_line", "ol", ol["order_id"] == o["id"])
    .group_by(date_format(o["created_at"], "%Y-%m"))
    .order_by(expr("month"), "asc")
