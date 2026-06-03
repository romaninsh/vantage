// v4 Q8: fuzzy string matching
// Tier 2: similarity (jaro_winkler), lower, words. The literal search term is inlined
// single-quoted by similarity(). Stubbed (expr): only the `similarity` order-by alias ref.
select()
    .from("client")
    .field("name")
    .expression(similarity(lower(ident("name")), "marti mcfligh").alias("similarity"))
    .expression(words(ident("name")).alias("name_parts"))
    .order_by(expr("similarity"), "desc")
    .limit(4, 0)
