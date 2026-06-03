// v4 Q4: correlated subquery with $parent — fully primitive-expressed (no expr() stubs).
// graph(me, "placed", "order") for the hop + ["total"] field tail; Tier 1 count, existing sum.
// The correlated (SELECT VALUE … GROUP ALL)[0] is a nested select().value()…group_all().subquery(),
// indexed with [0]; the correlation reads $parent.id via parent("id").
select()
    .from("client")
    .field("name")
    .field("tier")
    .expression(count(graph(me, "placed", "order")).alias("num_orders"))
    .expression(sum(graph(me, "placed", "order")["total"]).alias("lifetime_spend"))
    .expression(
        select().value()
            .expression(max(ident("total")))
            .from("order")
            .where(ident("client") == parent("id"))
            .group_all()
            .subquery()[0].alias("biggest_order")
    )
    .where(count(graph(me, "placed", "order")) > 0)
    .order_by(ident("lifetime_spend"), "desc")
