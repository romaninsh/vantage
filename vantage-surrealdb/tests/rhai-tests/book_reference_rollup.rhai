// The primitives page's record-reference roll-up.
surreal::select().field("name")
    .expression(surreal::max(surreal::expr("<~order.total")).alias("biggest_order"))
    .from("client")
