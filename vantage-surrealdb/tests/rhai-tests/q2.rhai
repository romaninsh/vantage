// Q2: Aggregates with GROUP BY and HAVING-like WHERE
// Tests: count, sum, group_by, expression aliases, arithmetic

let orders = surreal::ident("orders");

surreal::select()
    .from(orders)
    .expression(orders["status"])
    .expression(surreal::count().alias("total"))
    .expression(surreal::sum(orders["amount"]).alias("sum_amount"))
    .group_by(orders["status"])
    .order_by(surreal::expr("sum_amount"), "desc")
