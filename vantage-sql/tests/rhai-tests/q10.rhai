// Q10: Client Product Affinity (GROUP_CONCAT + DISTINCT)
// Features: group_concat(DISTINCT), three-way LEFT JOIN chain

let c = sql::ident("client").alias("c");
let o = sql::ident("client_order").alias("o");
let ol = sql::ident("order_line").alias("ol");
let p = sql::ident("product").alias("p");

sql::select()
    .from(c)
    .expression(c["name"].alias("client"))
    .expression(sql::count_distinct(o["id"]).alias("total_orders"))
    .expression(sql::group_concat(p["name"], true).alias("products_bought"))
    .expression(
        sql::coalesce(sql::sum(sql::mul(ol["quantity"], ol["price"])), sql::expr("0"))
            .alias("lifetime_value"))
    .left_join("client_order", "o", o["client_id"] == c["id"])
    .left_join("order_line", "ol", ol["order_id"] == o["id"])
    .left_join("product", "p", p["id"] == ol["product_id"])
    .group_by(c["id"])
    .group_by(c["name"])
    .order_by(sql::expr("lifetime_value"), "desc")
