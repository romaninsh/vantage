// E4: Missing closing bracket in indexer
let u = table("users").alias("u");
select().from(u).field(u["name)
