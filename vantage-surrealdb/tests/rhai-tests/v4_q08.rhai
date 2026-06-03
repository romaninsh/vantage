// v4 Q8: fuzzy string matching
// Tier 2: similarity (jaro_winkler), lower, words. The literal search term is inlined
// single-quoted by similarity(). The order-by references the `similarity` output alias via ident().
select()
    .from("client")
    .field("name")
    .expression(similarity(lower(ident("name")), "marti mcfligh").alias("similarity"))
    .expression(words(ident("name")).alias("name_parts"))
    .order_by(ident("similarity"), "desc")
    .limit(4, 0)
