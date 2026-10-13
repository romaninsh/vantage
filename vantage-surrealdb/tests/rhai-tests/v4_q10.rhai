// v4 Q10: SPLIT in a subquery + conditional banding
// Tier 1: surreal::count(), avg, round, surreal::case_when().when().else_().expr(). The inner
// SPLIT subquery is a nested surreal::select().split("tags").subquery() used as the FROM source;
// `tags AS tag` and the `product_count` order-by reference columns/aliases by name via
// surreal::ident().
surreal::select()
    .from(
        surreal::select()
            .expression(surreal::ident("tags").alias("tag"))
            .field("price")
            .from("product")
            .split("tags")
            .subquery())
    .field("tag")
    .expression(surreal::count().alias("product_count"))
    .expression(surreal::round(surreal::avg(surreal::ident("price"))).alias("avg_price"))
    .expression(
        surreal::case_when()
            .when(surreal::avg(surreal::ident("price")) >= 250, "premium")
            .when(surreal::avg(surreal::ident("price")) >= 150, "mid")
            .else_("value")
            .expr()
            .alias("price_band"))
    .group_by(surreal::ident("tag"))
    .order_by(surreal::ident("product_count"), "desc")
    .order_by(surreal::ident("tag"), "asc")
