// surreal::expr("…", [args]): scalars bind as parameters, and an ident or an
// expression nests as SurrealQL.
surreal::select()
    .from("product")
    .where(surreal::expr("price > {} AND name != {} AND ok = {} AND weight < {}", [15, "x", true, 2.5]))
    .where(surreal::expr("{} = {}", [surreal::ident("a"), surreal::expr("b")]))
