// Q7: Order Detail View (all orders with totals)
// Features: INNER JOIN + LEFT JOIN, COUNT/SUM aggregates, GROUP BY

let o = table("client_order").alias("o");
let c = table("client").alias("c");
let ol = table("order_line").alias("ol");

select()
    .from(o)
    .expression(o["id"].alias("order_id"))
    .expression(c["name"].alias("client"))
    .expression(o["created_at"].alias("placed"))
    .expression(count(ol["id"]).alias("lines"))
    .expression(sum(mul(ol["quantity"], ol["price"])).alias("order_total"))
    .expression(sum(ol["quantity"]).alias("total_items"))
    .inner_join("client", "c", c["id"] == o["client_id"])
    .left_join("order_line", "ol", ol["order_id"] == o["id"])
    .group_by(o["id"])
    .group_by(c["name"])
    .group_by(o["created_at"])
    .order_by(o["created_at"], "asc")
