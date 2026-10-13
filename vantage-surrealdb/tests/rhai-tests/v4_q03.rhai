// v4 Q3: GROUP BY with the math:: stats family
// Tier 1: surreal::count(), avg, round. Existing: sum, max. Tier 2: stddev. department.name via
// indexer. The order-by references the `payroll` output alias by name via surreal::ident().
surreal::select()
    .from("employee")
    .expression(surreal::ident("department")["name"].alias("department"))
    .expression(surreal::count().alias("headcount"))
    .expression(surreal::sum(surreal::ident("salary")).alias("payroll"))
    .expression(surreal::avg(surreal::ident("salary")).alias("avg_salary"))
    .expression(surreal::max(surreal::ident("salary")).alias("top_salary"))
    .expression(surreal::round(surreal::stddev(surreal::ident("salary"))).alias("salary_spread"))
    .group_by(surreal::ident("department"))
    .order_by(surreal::ident("payroll"), "desc")
