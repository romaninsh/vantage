// The primitives page's GROUP ALL select clause.
surreal::select().value().expression(surreal::max(surreal::ident("total"))).from("order").group_all()
