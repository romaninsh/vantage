// surreal::graph() arity 7 (anchor + 6 segments) → fn_graph7, the current overload ceiling.
surreal::select()
    .from("employee")
    .expression(surreal::graph(surreal::me, "a", "b", "c", "d", "e", "f").alias("path"))
