// Q4: Product Inventory Health (CASE WHEN/THEN/ELSE)
// Features: CASE expression, LEFT JOIN, GROUP BY

let p = table("product").alias("p");
let ol = table("order_line").alias("ol");

select()
    .from(p)
    .expression(p["name"].alias("product"))
    .expression(p["inventory_stock"].alias("stock"))
    .expression(coalesce(sum(ol["quantity"]), expr("0")).alias("units_sold"))
    .expression(
        case_when()
            .when(p["inventory_stock"] <= 5, expr("'CRITICAL'"))
            .when(p["inventory_stock"] <= 15, expr("'LOW'"))
            .when(p["inventory_stock"] <= 30, expr("'OK'"))
            .else_(expr("'PLENTY'"))
            .expr()
            .alias("stock_status"))
    .expression(
        case_when()
            .when(p["is_deleted"] == false, expr("'active'"))
            .else_(expr("'DELETED'"))
            .expr()
            .alias("active"))
    .left_join("order_line", "ol", ol["product_id"] == p["id"])
    .group_by(p["id"])
    .group_by(p["name"])
    .group_by(p["inventory_stock"])
    .group_by(p["is_deleted"])
    .order_by(expr("stock"), "asc")
