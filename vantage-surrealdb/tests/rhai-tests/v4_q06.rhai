// v4 Q6: aggregating graph-edge properties
// surreal::graph("reviewed", surreal::me) for the incoming edge-only <-reviewed + ["rating"] edge-property
// tail; Tier 1 count, avg; existing max, min.
surreal::select()
    .from("product")
    .field("name")
    .field("category")
    .expression(surreal::count(surreal::graph("reviewed", surreal::me)).alias("num_reviews"))
    .expression(surreal::avg(surreal::graph("reviewed", surreal::me)["rating"]).alias("avg_rating"))
    .expression(surreal::max(surreal::graph("reviewed", surreal::me)["rating"]).alias("best"))
    .expression(surreal::min(surreal::graph("reviewed", surreal::me)["rating"]).alias("worst"))
    .where(surreal::count(surreal::graph("reviewed", surreal::me)) > 0)
    .order_by(surreal::ident("avg_rating"), "desc")
