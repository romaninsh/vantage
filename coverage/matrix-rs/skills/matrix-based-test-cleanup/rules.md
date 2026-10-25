# Rules for combining and dropping tests

Each rule comes from a case the user decided. Add a rule only with the user's approval, with the
case that made it. The challenger cites these by number.

## R1. One test per behaviour

Different inputs to the same function, checked for the same kind of outcome, are cases of one
test, not separate tests. Escaping different characters is one behaviour.

*Case: vantage-api-client `graphql_select.rs`: quote/backslash escaping, control-character
escaping and non-ASCII pass-through were three tests; they are one.*

## R2. One setup, one call, plain asserts

A combined test builds its subject once and makes one call that carries every case (several
conditions on one query, several fields in one patch), then one plain assert per case. No helper
functions, loops, tables or closures when a few lines do the job. Repeating a simple line is
fine; cleverness isn't.

*Case: the first proposal for R1's case used a helper and a loop and was rejected; three
conditions on one query with three asserts was accepted.*

```rust
let query = launches()
    .with_condition(GraphqlCondition::eq("quoted", "say \"hi\" \\ now\nnext\tcol\r"))
    .with_condition(GraphqlCondition::eq("control", "a\u{0}b\u{1b}c\u{7f}"))
    .with_condition(GraphqlCondition::eq("unicode", "Zürich ✓"))
    .render().await.unwrap().query;
assert!(query.contains(r#"quoted: "say \"hi\" \\ now\nnext\tcol\r""#), "{query}");
assert!(query.contains(r#"control: "a\u0000b\u001Bc\u007F""#), "{query}");
assert!(query.contains(r#"unicode: "Zürich ✓""#), "{query}");
```

## R3. Same setup, same call: one test

Two tests that build the same fixture and make the same call, differing only in what changed
beforehand, become one test: apply every change, make the call once, assert each outcome.

*Case: vantage-diorama `refresh_updates_visible_rows_in_place` and `refresh_recounts_appended_rows`
became one refresh that sees a changed row and an appended row.*

## R4. Keep the strongest input of a class, drop the weaker

When one input proves everything another does on the same path (more digits, longer text, the
harder case), keep the strong one under a clear name and delete the weaker. Diagnostic value
("if the big one breaks, the small one narrows it down") is not a reason to keep a case.

*Case: vantage-sql `test_varchar_decimal` (`123456.789012`) was dropped for
`test_varchar_high_precision` (33 significant digits).*

## R5. One input per distinct path; read branches, not expected values

Keep one input per class that takes a different path: positive vs negative, scalar vs array, a
separate branch. Drop an input that takes a path already shown, even a classic boundary like zero
when the code treats it the same. To decide whether two inputs are the same case, reading the
code's branches is allowed. Expected values always come from the contract (docs, doc comments,
sibling behaviour), never from the implementation.

*Cases: vantage-types epoch pairs kept positive-with-nanos and negative, dropped zero.
vantage-mongodb kept one scalar and the array for "empty text and null clear", because
`coerce.rs` handles `""` once before any per-type parse, so `when` and `amount` were the same case.*

## R6. Look past the group

A group lists tests with overlapping coverage. Tests of the same behaviour elsewhere in the file
or crate count too. Before proposing, search for them and include them.

*Case: vantage-mongodb's group missed `declared_type_empty_text_clears`, a third test of the same
rule in the same file.*

## R7. Exact values, readable

Write expected values exactly, as raw strings when escaping would hide them. Where the exact format
isn't documented, keep the existing partial assertions (`starts_with`, `contains`) rather than
reading the implementation to make them exact.

## R8. Shared waits live in support

Waiting for asynchronous work uses the crate's shared support helpers, not hand-rolled
subscribe-and-wait code in each test. (Diorama's `settle` helper is planned:
`docs/superpowers/plans/2026-10-25-diorama-test-settle.md`. Until it lands, keep the existing
`wait_for_gen` calls.)
