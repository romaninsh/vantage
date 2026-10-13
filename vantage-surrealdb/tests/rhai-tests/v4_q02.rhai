// v4 Q2: recursive graph traversal
// surreal::recurse() wraps a surreal::graph() path; surreal::count(surreal::graph(...)) over
// the edge-only <-reports_to.
surreal::select()
    .from("employee")
    .expression(surreal::ident("name").alias("manager"))
    .expression(
        surreal::recurse(surreal::graph("employee", "reports_to", surreal::me), 1, 5)["name"]
            .alias("deepest_report"))
    .where(surreal::count(surreal::graph("reports_to", surreal::me)) > 0)
    .order_by(surreal::ident("name"), "asc")
