// graph() arity 7 (anchor + 6 segments) → fn_graph7, the current overload ceiling.
select()
    .from("employee")
    .expression(graph(me, "a", "b", "c", "d", "e", "f").alias("path"))
