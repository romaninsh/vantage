// Q9: Product Category Performance (GROUP BY + HAVING)
// Features: GROUP BY, HAVING, COALESCE, multiple aggregates

let p = sql::ident("product").alias("p");

sql::select()
    .from(p)
    .expression(sql::coalesce(p["sticker"], sql::expr("'uncategorized'")).alias("category"))
    .expression(sql::count(p["id"]).alias("product_count"))
    .expression(sql::sum(p["inventory_stock"]).alias("total_stock"))
    .expression(sql::avg(p["price"]).alias("avg_price"))
    .expression(sql::sum(p["calories"]).alias("total_calories"))
    .where(p["is_deleted"] == false)
    .group_by(p["sticker"])
    .having(sql::expr("COUNT({}) > 1", [p["id"]]))
    .order_by(sql::expr("total_stock"), "desc")
