// surreal::param(name) → $name — SurrealDB parameter reference (Rhai reserves `var`, so the
// primitive is `param`). Proves it parses and the string indexer adds the field tail.
surreal::select()
    .from("order")
    .field("id")
    .where(surreal::ident("client") == surreal::param("auth")["id"])
    .where(surreal::param("this")["status"] == "open")
