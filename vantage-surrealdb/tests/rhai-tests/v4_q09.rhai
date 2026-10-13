// v4 Q9: object functions
// Existing: sum. Tier 2: object_entries, object_values; nutrition.sugar via
// surreal::ident("nutrition")["sugar"].
surreal::select()
    .from("product")
    .field("name")
    .field("category")
    .expression(surreal::object_entries(surreal::ident("nutrition")).alias("facts"))
    .expression(surreal::sum(surreal::object_values(surreal::ident("nutrition"))).alias("total_macros"))
    .expression(surreal::ident("nutrition")["sugar"].alias("sugar"))
    .where(surreal::ident("nutrition")["sugar"] > 15)
    .order_by(surreal::ident("sugar"), "desc")
