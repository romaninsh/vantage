// Q3: Monthly Revenue Trend
// Features: date_format for time buckets, COUNT(DISTINCT), GROUP BY derived column

let o = sql::ident("client_order").alias("o");
let ol = sql::ident("order_line").alias("ol");

sql::select()
    .from(o)
    .expression(sql::date_format(o["created_at"], "%Y-%m").alias("month"))
    .expression(sql::count_distinct(o["id"]).alias("orders"))
    .expression(
        sql::coalesce(sql::sum(sql::mul(ol["quantity"], ol["price"])), sql::expr("0"))
            .alias("revenue"))
    .left_join("order_line", "ol", ol["order_id"] == o["id"])
    .group_by(sql::date_format(o["created_at"], "%Y-%m"))
    .order_by(sql::expr("month"), "asc")
