// The primitives page's SPLIT select clause.
surreal::select().field("price").from("product").split("tags")
