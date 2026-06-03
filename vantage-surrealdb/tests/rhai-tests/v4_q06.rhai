// v4 Q6: aggregating graph-edge properties
// graph("reviewed", me) for the incoming edge-only <-reviewed + ["rating"] edge-property
// tail; Tier 1 count, avg; existing max, min.
select()
    .from("product")
    .field("name")
    .field("category")
    .expression(count(graph("reviewed", me)).alias("num_reviews"))
    .expression(avg(graph("reviewed", me)["rating"]).alias("avg_rating"))
    .expression(max(graph("reviewed", me)["rating"]).alias("best"))
    .expression(min(graph("reviewed", me)["rating"]).alias("worst"))
    .where(count(graph("reviewed", me)) > 0)
    .order_by(expr("avg_rating"), "desc")
