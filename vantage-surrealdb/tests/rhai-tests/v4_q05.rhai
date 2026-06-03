// v4 Q5: closures over embedded arrays
// Entirely stubbed (expr): array::len, .map/.fold closures, object literal — Tier 3.
select()
    .from("order")
    .field("id")
    .expression(expr("array::len(lines)").alias("line_count"))
    .expression(expr("lines.map(|$l| { product: $l.product.name, subtotal: $l.quantity * $l.price })").alias("breakdown"))
    .expression(expr("lines.fold(0, |$acc, $l| $acc + ($l.quantity * $l.price))").alias("computed_total"))
    .order_by(expr("computed_total"), "desc")
    .limit(5, 0)
