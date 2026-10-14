// surreal::graph() arity 6 (anchor + 5 segments) → fn_graph6. Same-direction sugar.
surreal::select()
    .from("employee")
    .expression(surreal::graph(surreal::me, "a", "b", "c", "d", "e").alias("path"))
