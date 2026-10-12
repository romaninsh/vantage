// v4 Q5: closures over embedded arrays — native Rhai `|l| …` closures run
// symbolically. `#{…}` lowers to an object literal; `*`/`+` build SurrealQL.
// The emitted `$value`/`$acc` are engine-chosen (Rhai locals can't carry `$`).
surreal::select()
    .from("order")
    .field("id")
    .expression(surreal::len(surreal::ident("lines")).alias("line_count"))
    .expression(
        surreal::ident("lines").map(|l| #{
            product: l["product"]["name"],
            subtotal: l["quantity"] * l["price"]
        }).alias("breakdown"))
    .expression(
        surreal::ident("lines")
            .fold(0, |acc, l| acc + l["quantity"] * l["price"])
            .alias("computed_total"))
    .order_by(surreal::ident("computed_total"), "desc")
    .limit(5, 0)
