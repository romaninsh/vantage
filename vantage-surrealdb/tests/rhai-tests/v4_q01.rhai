// v4 Q1: graph traversal + null-coalescing
// surreal::graph(surreal::me, ...) for the ->reports_to->employee hop + ["name"] field tail;
// Tier 1 surreal::coalesce + Tier 2 surreal::first(); department.name via
// surreal::ident("department")["name"]. No surreal::expr() stubs.
surreal::select()
    .from("employee")
    .field("name")
    .field("title")
    .field("salary")
    .expression(surreal::ident("department")["name"].alias("department"))
    .expression(
        surreal::coalesce(
            surreal::first(surreal::graph(surreal::me, "reports_to", "employee")["name"]),
            "n/a"
        ).alias("manager"))
    .order_by(surreal::ident("salary"), "desc")
