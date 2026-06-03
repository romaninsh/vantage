// v4 Q2: recursive graph traversal
// recurse() wraps a graph() path; count(graph(...)) over the edge-only <-reports_to.
select()
    .from("employee")
    .expression(ident("name").alias("manager"))
    .expression(recurse(graph("employee", "reports_to", me), 1, 5)["name"].alias("deepest_report"))
    .where(count(graph("reports_to", me)) > 0)
    .order_by(ident("name"), "asc")
