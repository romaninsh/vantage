// graph() arity 6 (anchor + 5 segments) → fn_graph6. Same-direction sugar.
select()
    .from("employee")
    .expression(graph(me, "a", "b", "c", "d", "e").alias("path"))
