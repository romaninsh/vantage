// Q4: Product Inventory Health (CASE WHEN/THEN/ELSE)
// Features: CASE expression, LEFT JOIN, GROUP BY

let p = sql::ident("product").alias("p");
let ol = sql::ident("order_line").alias("ol");

sql::select()
    .from(p)
    .expression(p["name"].alias("product"))
    .expression(p["inventory_stock"].alias("stock"))
    .expression(sql::coalesce(sql::sum(ol["quantity"]), sql::expr("0")).alias("units_sold"))
    .expression(
        sql::case_when()
            .when(p["inventory_stock"] <= 5, sql::expr("'CRITICAL'"))
            .when(p["inventory_stock"] <= 15, sql::expr("'LOW'"))
            .when(p["inventory_stock"] <= 30, sql::expr("'OK'"))
            .else_(sql::expr("'PLENTY'"))
            .expr()
            .alias("stock_status"))
    .expression(
        sql::case_when()
            .when(p["is_deleted"] == false, sql::expr("'active'"))
            .else_(sql::expr("'DELETED'"))
            .expr()
            .alias("active"))
    .left_join("order_line", "ol", ol["product_id"] == p["id"])
    .group_by(p["id"])
    .group_by(p["name"])
    .group_by(p["inventory_stock"])
    .group_by(p["is_deleted"])
    .order_by(sql::expr("stock"), "asc")
