# Rules for combining and dropping tests

R1–R11 come from cases the user decided. Rules the two agents agree during a run go under
"Agreed in runs" at the end, with their case; the user reviews them from the run's report. The
challenger cites rules by number.

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

## R9. One call per branch that stops early

When a function returns at its first error, one input can't reach two of its error branches.
The cases still make one test (R1): one setup, then one call per such branch, each a plain line
with its assert. This is the only exception to R2's single call.

*Case: vantage-types `#[entity]` `from_record` stops at "Missing field" before it can reach
"Failed to convert field"; a missing field and a wrongly typed field are two calls in one test.*

## R10. Cases over the same machinery live together

Inputs that run through the same machinery belong in one test, even when the tests reach it
through different entry points (a function directly, or through a dialect or wrapper that calls
it). A second entry point is a different behaviour only when the entry point adds something of its
own (routing, a different output), and then the test asserts that addition.

*Case: vantage-types epoch pairs: the positive pair was rendered through `PresentationDialect` and
the negative pair through `tag12_to_rfc3339` directly, in two tests. Same rendering, so the
negative pair joins the presentation test.*

## R11. A difference must be shown, not only stated

Every case kept for being different must have asserts that would fail if it weren't. Two inputs
kept because they take different branches need asserts that tell the branches apart (the error
message or kind, where the contract names it). Two bare `is_err()` asserts show one branch twice.

*Case: vantage-types `entity_rejects_a_record_missing_or_mistyping_a_field` kept a missing field
and a mistyped field for their different branches, but asserted only `is_err()` on both. Where
the contract doesn't name the messages, `assert_ne!` on them shows the branches differ.*

## R12. Identical coverage isn't a duplicate by itself: a legit case gets an exception

Not every duplicate the matrix or CI's duplicate check reports must be resolved. Two tests that
run the same code in this crate but check different behaviours (different calls, different
promises, their asserts showing the difference per R11) stay as two tests. They aren't merged
into one test just to satisfy the check. The proposal names each such test and gives the exact
`coverage/vend-exception.sh <package> <test id> "<reason>"` command; the user vends it. Tests that
check the same behaviour still merge or go (R1, R3).

*Case: vantage-sql #478. CI paired the sqlite vista metadata tests (capability flags, Orderable
columns, a refused `set_order`) because each only builds a `from_table` vista in vantage-sql. The
user ruled they stay apart with exceptions rather than become one test.*

## Agreed in runs

### A1. Sibling entry points: one call each, one test

Sibling entry points over the same machinery (R10) that each add an output of their own can't
share one call. They go in one test: one setup, then one plain line per entry point, each
asserting its own distinct value. This is a second exception to R2's single call, next to R9.

*Case: vantage-sql mysql `4_aggregates.rs`: `test_count`, `test_sum_price`, `test_max_price`,
`test_min_price` became `test_aggregates`, with one `get_count` / `get_sum` / `get_max` /
`get_min` line each (5, 973, 299, 120).*

### A2. One result per call: one call per case

When the public API under test returns a single result per call, cases that need different
results (different rows) can't share one call. They go in one test: one setup, one call per
case, each with its own plain assert. Use this only when no public form of the call carries
several cases at once.

*Case: vantage-sql mysql `2_records.rs`: `associate::<Record<JsonValue>>().get()` returns one row,
so `test_select_single_record_into_entity` (cupcake, every column set) and
`test_null_into_optional_field` (plain, NULLs → None) became one test with two fetches and two
exact `Product` equalities.*

### A4. Repetition or a separate subject: one call each, one test

Some cases can't share one call. Either repeating the call is the behaviour (a slot evaluated
against a second env, a second compile that must hit or miss the cache, a second batch through
one pipe), or each case needs its own subject (hosts built with different limits). These cases go
in one test: one setup, then one plain line per repetition or subject, each with its assert.
Every case that can share a call still does, and the proposal names A4 for each extra call.
This is another exception to R2's single call, next to R9, A1 and A2. A2 covers different inputs
that each need their own result. A4 covers calls whose repetition is the point, or cases that need
another subject.

*Case: vantage-rhai `host.rs` `each_host_stops_work_past_its_operation_ceiling`: the UI, background
and zero-ceiling hosts get one call each. Background's runaway loop is a second call by R9. The same
rule allowed the second `eval` in `a_compiled_slot_evaluates_against_whichever_env_it_is_given` and
`the_resolver_is_asked_once_per_read_on_every_evaluation`, the paired `ast`/`compile` calls in the
cache tests, and the second batch in `rows_are_answered_in_order_by_one_script_that_keeps_its_state`.*

### A5. One error by design: each branch keeps its own call

When the contract deliberately gives several branches the same error, each branch still keeps its
own plain call and assert in one test, with no loop. The documented single message satisfies R11:
no `assert_ne!` can tell those branches apart, and cutting them to one input would lose the
branches. (A user ruling.)

*Case: vantage-mongodb `6_vista_paging.rs` `vista_fetch_cursor_refuses_tokens_it_did_not_issue`:
`decode_token` refuses every foreign token with "invalid fetch_cursor token". The integer, `[]`,
`[null]` and `[bytes 1,2,3]` tokens each take a different early return, so each keeps its own
plain `fetch_cursor` call asserting that message.*

### A6. A branch the contract names but can't isolate keeps one input

Sometimes the contract names a condition as its own rule but gives it the same documented output
as other conditions, and every input that meets it also meets one of the others. No assert can
show that condition alone. It then keeps one input, the one that reaches its branch, with its own
plain call and an exact assert of the documented output; that output satisfies R11. This is A5
(the same error from several branches) for the same output. A second input on the same branch is
still a weaker case (R4, R5).

*Case: surreal-client `record.rs` `escape_identifier`. The doc wraps a name in `⟨…⟩` when it is
"numeric" or "starts with a digit", among other conditions. Every numeric name is also
digit-leading (`123`, `1.5`) or holds a non-identifier character (`-7`), so the numeric early
return gives the same `⟨…⟩`. `123` stays as the one input reaching that return; `1.5` was dropped
as a second input on the same branch; `1abc` shows the digit rule.*
