// Q3: SurrealDB graph traversal
// Tests: arrow (->), back (<-), graph field access

let user = surreal::ident("user");

surreal::select()
    .from(user)
    .field("name")
    .expression(user.arrow("orders").alias("order_ids"))
    .expression(user.arrow_field("orders", "id").alias("order_list"))
