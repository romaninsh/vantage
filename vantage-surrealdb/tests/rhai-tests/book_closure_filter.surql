SELECT lines.filter(|$value| $value.qty > 0) AS filled, lines.map(|$value| [$value.product, $value.qty]) AS pairs FROM order
