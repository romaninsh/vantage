// v4 Q9: object functions
// Existing: sum. Tier 2: object_entries, object_values; nutrition.sugar via ident("nutrition")["sugar"].
select()
    .from("product")
    .field("name")
    .field("category")
    .expression(object_entries(ident("nutrition")).alias("facts"))
    .expression(sum(object_values(ident("nutrition"))).alias("total_macros"))
    .expression(ident("nutrition")["sugar"].alias("sugar"))
    .where(ident("nutrition")["sugar"] > 15)
    .order_by(ident("sugar"), "desc")
