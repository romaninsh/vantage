# README code snippets contain Rust syntax errors and typos

- **Severity:** low
- **Category:** bugs
- **Location:** `README.md:35` (also lines 130, 230, 416, 452, 470-471)

Several README snippets are not valid Rust even as pseudo-code: line 35 `email: Email(String),` is not a legal field type; line 130 `fn new(){ /* ... */};` has a stray semicolon and missing signature; line 230 `let entities: = vec![clients, orders, ..];` has an empty type ascription; line 416 `let now = if Some(date) = cutoff_date` is missing `let` in the `if let`; lines 470-471 have unbalanced parens and a string opened with `"` but closed with `'`, plus typos `postgrs` and (line 452) `eu_countires`. For a crate whose pitch is type-safety and ergonomics, the front-page examples failing to even parse undermines credibility.

```
    .with_expression(expr!("SUM(vat)", Some("total_vat".to_string()))
    .with_condition(expr!("country_code in {}', postgrs.defer(&eu_countries)))
    .get().await?; // <-- single await!
```

**Recommendation:** Run the snippets through `rustfmt`/a doctest-style compile pass (or source them from the compiled `learn-*` crates as the book does) and fix the typos.
