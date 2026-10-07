# Layers

The data vocabulary acts on Vistas and nothing else. Three parts of the framework sit above Vista
and give scripts words of their own: Servo drafts in `vantage-diorama`, Scenery shapes in Vantage
UI, and faker sims. This chapter shows how each one relates to the data vocabulary, and why they
stay separate.

<!-- toc -->

---

## The rule

`vantage-vista` never refers to Dio, Servo or Scenery. Each upper layer either hands scripts a plain
Vista or registers its own vocabulary next to `DataVocab` on the same host. A host for a form's
`on_submit` script, for example, registers both `ServoVocab` and `DataVocab`, so the script has
`form` (a Servo) and `table(name)` (Vistas) side by side.

Where an upper layer means the same thing as a data verb, it uses the same word: `where`, `sort`,
`limit`, `ref`, `save`, `is_dirty`. Where it means something different, it uses its own word, even
if a data verb looks close.

## Dio: no vocabulary of its own

A Dio doesn't add script words. [`Dio::vista()`](vantage_diorama::Dio::vista) returns a Vista whose
reads come from the Dio's cache and whose writes are queued as flashes and written through to the
master. A host that resolves `table(name)` to Dio Vistas gives scripts cached reads and optimistic
writes, and the scripts are the same ones that would run against the bare backend. Vantage UI's
action bodies work this way.

## Servo: a form draft

A [Servo](vantage_diorama::Servo) is a draft of one record bound to a Dio. It tracks a baseline,
staged edits, a status and per-field rejections from the Dio's flash route. Forms and wizards in
Vantage UI hand one to their scripts as `form`. `ServoVocab` (`vantage_diorama::rhai`) registers
its words:

<!-- tested: vantage-diorama rhai_servo::set_save_settles_a_new_record -->
```rhai
servo.set("id", "tag:AB12-CD34");
servo.set("status", "unregistered");
if !servo.is_dirty() { throw "draft should be dirty"; }
if !servo.dirty("status") { throw "field should be dirty"; }
servo.save()
```

A Servo and a data-vocabulary [record](./records.md) look alike, and some words match:

| | record (`table(t).record(id)`) | Servo |
|---|---|---|
| read a field | `r.col`, `r["col"]` | `servo.get("col")` |
| stage a field | `r.col = v`, `r.set(map)` | `servo.set("col", v)` |
| id | `r.id` | `servo.id()` |
| id of a new row | a UUIDv7 minted on the first save | [`IdStrategy`](vantage_diorama::IdStrategy): `Uuid` (the default) mints one when the Servo opens |
| the whole draft | (field reads) | `servo.record()` |
| baseline | `r.baseline()` | `servo.baseline()` |
| dirty | `r.is_dirty()`, `r.dirty(col)` | `servo.is_dirty()`, `servo.dirty(col)` |
| revert | `r.revert(col)`, `r.revert()` | `servo.revert(col)`, `servo.revert_all()` |
| fields that differ from the baseline | | `servo.error()` |
| save | `r.save()`: a patch or insert, now | `servo.save()`: a flash, waits for it to settle |
| status | `r.status()`, `r.rejection()` | `servo.status()`, `servo.rejection()` with per-field errors |

They stay separate because they are different objects. A record is a script's own short-lived
draft over any Vista, and is gone when the script ends. A Servo lives in the Dio and outlasts the
script. The form's widgets are bound to it, its flash goes through the Dio's write queue and route,
and a route can reject single fields. Folding the two into one type would make the record depend
on the Dio, or hide the Servo's lifecycle behind data-vocabulary words.

`servo.save()` needs a tokio runtime context and blocks on the flash, so Servo scripts run under
`spawn_blocking`, like any data script with a network backend.

### Which one to use

Use a record for a script's own one-off edit over any Vista; it saves when the script says so and
is gone after. Use a [Servo](vantage_diorama::Servo) when a person edits: it lives in the Dio,
outlasts the script, takes per-field rejections, and its save goes through the flash route.

Unless the id is left to the backend (an `auto` id column, `IdStrategy::Auto`), both reuse their id
when a save is retried, so neither can create a row twice (see
[Safe writes](../record-lifecycle.md#safe-writes)).

## Scenery: the shape of a view

A Scenery is a live, reactive view over a Dio: a [`TableScenery`](vantage_diorama::TableScenery)
keeps an ordered window of rows current as the Dio's cache changes, a
[`ValueScenery`](vantage_diorama::ValueScenery) keeps one aggregate current, and a
[`RecordScenery`](vantage_diorama::RecordScenery) one row (see
[Scenery — Reactive Views](../intro/step7-scenery.md)). An application binds a widget to one.

`vantage-diorama` has no Scenery script words. Vantage UI adds some: a component's `table:` binding
can be a plain table key or a short script, `scenery(name)` followed by narrowing words, that
describes which Scenery to open. The script builds a description, and the page opens the Scenery
when it mounts:

<!-- tested: vantage-ui scenery_script::tests::chain_builds_a_spec -->
```rhai
scenery("top").sort("visitors", "desc").limit(10)
```

A child view through a relation, for the row selected in a grid:

<!-- tested: vantage-ui scenery_script::tests::scenery_ref_builds_the_same_related_shape -->
```rhai
scenery("launches").where("id", "42").ref("payloads")
```

In a page this reads `launches_grid.selected_id` instead of `"42"`, and the page remounts the
binding when the selection changes.

Shared words, with the same meaning as on a table handle:

- `where(col, value)`: equality only. Values carry as text, so a page number needs no
  `to_string()`.
- `sort(col, dir)`: the direction is required, `"asc"` or `"desc"`.
- `limit(n)`: at most `n` rows.
- `ref(relation)`: the related rows. On a Scenery it must follow exactly one `where` on the
  parent's id and nothing else, because the binding follows one parent row.

Scenery-only words:

- `tail(n)`: follow the last `n` rows as they arrive, for append-only feeds.
- `arg(name, value)` and `args(#{ … })`: parameters passed to the table's own `rhai:` query
  script as its `args` map, which reshape the query itself rather than filtering its result (see
  [Arguments](./query-tables.md#arguments)).

Scenery has no reads or writes: it describes a view, and the page's Dio opens it. The vocabulary
lives in Vantage UI (`SceneryVocab` in `crates/app/src/infra/scenery_script.rs`), runs under
`Limits::Ui`, and records every page value the script reads as a dependency of the binding.

## Faker sims

Sims use the data vocabulary over a memory store plus time and random words. See
[Faker Sims](./faker.md).
