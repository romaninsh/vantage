// v4 Q8: fuzzy string matching
// Tier 2: similarity (jaro_winkler), lower, words. The literal search term is bound
// as a parameter by surreal::similarity(). The order-by references the `similarity` output alias
// via surreal::ident().
surreal::select()
    .from("client")
    .field("name")
    .expression(
        surreal::similarity(surreal::lower(surreal::ident("name")), "marti mcfligh")
            .alias("similarity"))
    .expression(surreal::words(surreal::ident("name")).alias("name_parts"))
    .order_by(surreal::ident("similarity"), "desc")
    .limit(4, 0)
