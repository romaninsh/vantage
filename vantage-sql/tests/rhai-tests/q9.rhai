// Q9: Product Category Performance (GROUP BY + HAVING)
// Features: GROUP BY, HAVING, COALESCE, multiple aggregates

let p = table("product").alias("p");

select()
    .from(p)
    .expression(coalesce(p["sticker"], expr("'uncategorized'")).alias("category"))
    .expression(count(p["id"]).alias("product_count"))
    .expression(sum(p["inventory_stock"]).alias("total_stock"))
    .expression(avg(p["price"]).alias("avg_price"))
    .expression(sum(p["calories"]).alias("total_calories"))
    .where(p["is_deleted"] == false)
    .group_by(p["sticker"])
    .having(expr("COUNT({}) > 1", [p["id"]]))
    .order_by(expr("total_stock"), "desc")
