// Q2: Client Spending Summary
// Features: Multiple LEFT JOINs, COUNT(DISTINCT), COALESCE, ROUND, AVG

let client = table("client").alias("c");
let client_order = table("client_order").alias("o");
let order_line = table("order_line").alias("ol");

select()
    .from(client)
    .expression(client["name"].alias("client"))
    .expression(
        coalesce(sum(mul(order_line["quantity"], order_line["price"])), expr("0.0"))
            .alias("total_spent"))
    .expression(
        count_distinct(client_order["id"])
            .alias("order_count"))
    .expression(
        round(
            div(cast(sum(mul(order_line["quantity"], order_line["price"])), "REAL"), nullif(count_distinct(client_order["id"]), expr("0"))),
            2
        ).alias("avg_order"))
    .left_join("client_order", "o", client_order["client_id"] == client["id"])
    .left_join("order_line", "ol", order_line["order_id"] == client_order["id"])
    .group_by(client["id"])
    .group_by(client["name"])
    .order_by(expr("total_spent"), "desc")
