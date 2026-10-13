// E4: Missing closing bracket in indexer
let u = sql::ident("users").alias("u");
sql::select().from(u).field(u["name)
