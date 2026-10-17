// Embedded-array closures from the primitives page: `filter` takes a native
// closure, and a Rhai `[…]` array in a body lowers to an array literal.
surreal::select()
    .from("order")
    .expression(surreal::ident("lines").filter(|l| l["qty"] > 0).alias("filled"))
    .expression(surreal::ident("lines").map(|l| [l["product"], l["qty"]]).alias("pairs"))
