// The primitives page's record-reference roll-up.
surreal::select().field("name")
    .expression(surreal::max(surreal::expr("<~(order FIELD client).total")).alias("biggest_order"))
    .from("client")
