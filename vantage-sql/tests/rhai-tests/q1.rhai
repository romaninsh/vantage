// Q1: Basic user query with filtering and ordering
// Tests: select, from, where, group_by, order_by, limit, expressions, operators

let users = table("users").alias("u");

select()
    .from(users)
    .expression(users["id"])
    .expression(users["name"])
    .expression(sum(users["salary"]).alias("total_salary"))
    .expression(avg(users["age"]).alias("avg_age"))
    .where(users["active"] == true)
    .where(users["department_id"] != 0)
    .group_by(users["id"])
    .group_by(users["name"])
    .having(sum(users["salary"]) > 50000)
    .order_by(users["name"], "asc")
    .order_by(expr("total_salary"), "desc")
    .limit(10, 0)
