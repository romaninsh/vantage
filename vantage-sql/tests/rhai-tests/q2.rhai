// Q2: Client Spending Summary
// Features: Multiple LEFT JOINs, COUNT(DISTINCT), COALESCE, ROUND, AVG

let client = sql::ident("client").alias("c");
let client_order = sql::ident("client_order").alias("o");
let order_line = sql::ident("order_line").alias("ol");

sql::select()
    .from(client)
    .expression(client["name"].alias("client"))
    .expression(
        sql::coalesce(sql::sum(sql::mul(order_line["quantity"], order_line["price"])), sql::expr("0.0"))
            .alias("total_spent"))
    .expression(
        sql::count_distinct(client_order["id"])
            .alias("order_count"))
    .expression(
        sql::round(
            sql::div(
                sql::cast(sql::sum(sql::mul(order_line["quantity"], order_line["price"])), "REAL"),
                sql::nullif(sql::count_distinct(client_order["id"]), sql::expr("0"))),
            2
        ).alias("avg_order"))
    .left_join("client_order", "o", client_order["client_id"] == client["id"])
    .left_join("order_line", "ol", order_line["order_id"] == client_order["id"])
    .group_by(client["id"])
    .group_by(client["name"])
    .order_by(sql::expr("total_spent"), "desc")
