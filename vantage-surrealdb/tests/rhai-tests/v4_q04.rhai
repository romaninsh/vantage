// v4 Q4: correlated subquery with $parent — fully primitive-expressed (no surreal::expr() stubs).
// surreal::graph(surreal::me, "placed", "order") for the hop + ["total"] field tail; Tier 1
// count, existing sum. The correlated (SELECT VALUE … GROUP ALL)[0] is a nested
// surreal::select().value()…group_all().subquery(), indexed with [0]; the correlation reads
// $parent.id via surreal::parent("id").
surreal::select()
    .from("client")
    .field("name")
    .field("tier")
    .expression(surreal::count(surreal::graph(surreal::me, "placed", "order")).alias("num_orders"))
    .expression(
        surreal::sum(surreal::graph(surreal::me, "placed", "order")["total"])
            .alias("lifetime_spend"))
    .expression(
        surreal::select().value()
            .expression(surreal::max(surreal::ident("total")))
            .from("order")
            .where(surreal::ident("client") == surreal::parent("id"))
            .group_all()
            .subquery()[0].alias("biggest_order")
    )
    .where(surreal::count(surreal::graph(surreal::me, "placed", "order")) > 0)
    .order_by(surreal::ident("lifetime_spend"), "desc")
