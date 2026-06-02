// Q4: SurrealDB ONLY + record ID + VALUE
// Tests: only, thing, value

let id = thing("user", "tobie");

select()
    .only()
    .value()
    .from(id)
    .field("name")
    .field("email")
