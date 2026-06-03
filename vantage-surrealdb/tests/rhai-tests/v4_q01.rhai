// v4 Q1: graph traversal + null-coalescing
// graph(me, ...) for the ->reports_to->employee hop + ["name"] field tail; Tier 1
// coalesce + Tier 2 first(); department.name via ident("department")["name"]. No expr() stubs.
select()
    .from("employee")
    .field("name")
    .field("title")
    .field("salary")
    .expression(ident("department")["name"].alias("department"))
    .expression(coalesce(first(graph(me, "reports_to", "employee")["name"]), "n/a").alias("manager"))
    .order_by(ident("salary"), "desc")
