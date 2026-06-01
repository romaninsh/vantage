// Q10: Client Product Affinity (GROUP_CONCAT + DISTINCT)
// Features: group_concat(DISTINCT), three-way LEFT JOIN chain

let c = table("client").alias("c");
let o = table("client_order").alias("o");
let ol = table("order_line").alias("ol");
let p = table("product").alias("p");

select()
    .from(c)
    .expression(c["name"].alias("client"))
    .expression(count_distinct(o["id"]).alias("total_orders"))
    .expression(group_concat(p["name"], true).alias("products_bought"))
    .expression(coalesce(sum(mul(ol["quantity"], ol["price"])), expr("0")).alias("lifetime_value"))
    .left_join("client_order", "o", o["client_id"] == c["id"])
    .left_join("order_line", "ol", ol["order_id"] == o["id"])
    .left_join("product", "p", p["id"] == ol["product_id"])
    .group_by(c["id"])
    .group_by(c["name"])
    .order_by(expr("lifetime_value"), "desc")
