// Q2: Aggregates with GROUP BY and HAVING-like WHERE
// Tests: count, sum, group_by, expression aliases, arithmetic

let orders = table("orders");

select()
    .from(orders)
    .expression(orders["status"])
    .expression(count().alias("total"))
    .expression(sum(orders["amount"]).alias("sum_amount"))
    .group_by(orders["status"])
    .order_by(expr("sum_amount"), "desc")
