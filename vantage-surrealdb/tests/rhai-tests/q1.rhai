// Q1: Basic SELECT with WHERE, ORDER BY, LIMIT
// Tests: select, from, field, where, order_by, limit, comparison operators

let users = table("users");

select()
    .from(users)
    .field("name")
    .field("age")
    .where(users["active"] == true)
    .order_by(users["name"], "asc")
    .limit(10, 0)
