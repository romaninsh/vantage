// Q7: Order Detail View (all orders with totals)
// Features: INNER JOIN + LEFT JOIN, COUNT/SUM aggregates, GROUP BY

let o = sql::ident("client_order").alias("o");
let c = sql::ident("client").alias("c");
let ol = sql::ident("order_line").alias("ol");

sql::select()
    .from(o)
    .expression(o["id"].alias("order_id"))
    .expression(c["name"].alias("client"))
    .expression(o["created_at"].alias("placed"))
    .expression(sql::count(ol["id"]).alias("lines"))
    .expression(sql::sum(sql::mul(ol["quantity"], ol["price"])).alias("order_total"))
    .expression(sql::sum(ol["quantity"]).alias("total_items"))
    .inner_join("client", "c", c["id"] == o["client_id"])
    .left_join("order_line", "ol", ol["order_id"] == o["id"])
    .group_by(o["id"])
    .group_by(c["name"])
    .group_by(o["created_at"])
    .order_by(o["created_at"], "asc")
