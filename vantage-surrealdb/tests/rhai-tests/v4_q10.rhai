// v4 Q10: SPLIT in a subquery + conditional banding
// Tier 1: count(), avg, round, case_when().when().else_().expr(). The inner SPLIT subquery is a
// nested select().split("tags").subquery() used as the FROM source; `tags AS tag` and the
// `product_count` order-by reference columns/aliases by name via ident().
select()
    .from(
        select()
            .expression(ident("tags").alias("tag"))
            .field("price")
            .from("product")
            .split("tags")
            .subquery())
    .field("tag")
    .expression(count().alias("product_count"))
    .expression(round(avg(ident("price"))).alias("avg_price"))
    .expression(
        case_when()
            .when(avg(ident("price")) >= 250, "premium")
            .when(avg(ident("price")) >= 150, "mid")
            .else_("value")
            .expr()
            .alias("price_band"))
    .group_by(ident("tag"))
    .order_by(ident("product_count"), "desc")
    .order_by(ident("tag"), "asc")
