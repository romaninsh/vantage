// Q4: SurrealDB ONLY + record ID + VALUE
// Tests: only, thing, value

let id = surreal::thing("user", "tobie");

surreal::select()
    .only()
    .value()
    .from(id)
    .field("name")
    .field("email")
