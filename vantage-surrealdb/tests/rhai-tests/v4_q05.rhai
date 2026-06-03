// v4 Q5: closures over embedded arrays — native Rhai `|l| …` closures run
// symbolically. `#{…}` lowers to an object literal; `*`/`+` build SurrealQL.
// The emitted `$value`/`$acc` are engine-chosen (Rhai locals can't carry `$`).
select()
    .from("order")
    .field("id")
    .expression(len(ident("lines")).alias("line_count"))
    .expression(
        ident("lines").map(|l| #{
            product: l["product"]["name"],
            subtotal: l["quantity"] * l["price"]
        }).alias("breakdown"))
    .expression(
        ident("lines")
            .fold(0, |acc, l| acc + l["quantity"] * l["price"])
            .alias("computed_total"))
    .order_by(ident("computed_total"), "desc")
    .limit(5, 0)
