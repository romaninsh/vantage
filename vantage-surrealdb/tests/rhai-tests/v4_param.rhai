// param(name) → $name — SurrealDB parameter reference (Rhai reserves `var`, so the
// primitive is `param`). Proves it parses and the string indexer adds the field tail.
select()
    .from("order")
    .field("id")
    .where(ident("client") == param("auth")["id"])
    .where(param("this")["status"] == "open")
