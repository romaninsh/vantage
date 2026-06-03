// v4 Q3: GROUP BY with the math:: stats family
// Tier 1: count(), avg, round. Existing: sum, max. Tier 2: stddev. department.name via indexer.
select()
    .from("employee")
    .expression(ident("department")["name"].alias("department"))
    .expression(count().alias("headcount"))
    .expression(sum(ident("salary")).alias("payroll"))
    .expression(avg(ident("salary")).alias("avg_salary"))
    .expression(max(ident("salary")).alias("top_salary"))
    .expression(round(stddev(ident("salary"))).alias("salary_spread"))
    .group_by(ident("department"))
    .order_by(expr("payroll"), "desc")
