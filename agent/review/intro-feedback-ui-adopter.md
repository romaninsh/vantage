# Intro review — vantage-ui adopter persona

Reviewer stance: I lead a small platform team. We liked Vantage UI (the product) and I'm
evaluating the open framework underneath before we commit — because when it breaks at 2am,
I'm the one holding the pager, and we will need to attach at least one in-house data source.
I read every page top to bottom. I read Rust fine; I'm not a trait-solver lawyer.

## What vantage-ui.com promised me

- "AI-first low-code App Builder": connect SQL, SurrealDB, MongoDB, GraphQL, REST, AWS, CLI
  tools; an AI agent authors YAML config over a local MCP server.
- "Million-row tables, reactive" with on-screen-only row fetching; live updates on YAML or
  data change without rebuild.
- /framework page: **nine** layered abstractions (DataSources → DataSet → Table → Vista →
  Vista Factory → Diorama → Lens → Scenery → integration), an explicit capability system,
  "database-agnostic expression AST with push-down", generation counters for reactivity,
  "roll your own" data sources via a nine-step persistence guide, and "production code: it
  builds Vantage UI itself" — at version "0.5.x".

So I came to verify: the layer model, the capability contract, the reactive machinery behind
the million-row grid, and the extension path for custom sources. The YAML/Rhai config path is
what the *product actually runs on* (the AI agent writes YAML, not Rust), so I also came to
understand that.

## Overall verdict

The guide is substantially better than most pre-1.0 framework docs. By the end I genuinely
understand Table → Vista → Dio/Lens → Scenery, the capability contract is the single best
idea in here and it's explained honestly, and chapters 5–8 demonstrate the machinery behind
the product's headline features (cached listings, viewport-driven hydration, watch streams,
one-flight-per-row) with real numbers instead of adjectives. The S3 resume-from-last-key trick
and the two-watch scheduler demo in step 8 are exactly the kind of proof I evaluate for.

What I do **not** trust yet:

1. **The config-driven path is a black box.** Vantage UI is driven by YAML + Rhai via an AI
   agent — that is the product I'd deploy. The guide walks the *typed Rust* path for eight
   chapters and waves at "Config-Driven Vistas" four separate times without ever showing ten
   lines of YAML. When the AI agent writes a broken table spec at my company, the thing I'll
   debug is a VistaFactory materialization — and this guide taught me nothing about it.
2. **The vantage-aws table-name DSL is the leakiest abstraction in the book.**
   `"restxml/Contents@continuation-token=NextContinuationToken:s3/GET /{Bucket}?list-type=2"`
   is an undocumented micro-language stuffed into a string that everywhere else means "a table
   name". No grammar, no error story for a typo, no pointer to a reference. This is the exact
   pattern I'd have to imitate to wrap our internal APIs, and I can't.
3. **Failure modes are thin.** `WriteFailed` gets one sentence; reconciliation conflict
   semantics ("edits reconcile instead of clobbering" from the intro) are never demonstrated;
   nothing on redb file locking across processes, cache corruption, master schema drift, watch
   backpressure, or the memory bound of a Dio holding a 122k-row spine (or the site's promised
   million rows).
4. **Site and book disagree on the basics**: nine layers vs four, 0.5.x vs 0.6, and step 3's
   Cargo.toml uses `path = "../vantage-core"` deps — which tells me the docs aren't tested
   against the published crates. For a foundation pitch, that's a trust dent.

Would I sign my team up? For a spike, yes — the architecture is coherent and the honesty
culture (capabilities, explicit errors, real timings) is what I want under a product. For
production adoption, not until I've read `new-persistence.md` and `config-driven-vistas.md`,
because the intro deliberately routed my two most important questions there.

## Per-page feedback

### introduction.md

- **Unclear:**
  - "it reconciles with the source over time" — over *what* time, by *what* rule? This is the
    load-bearing sentence of live mode and it's never resolved, not even by chapter 8. What
    happens when my local edit and a remote edit collide?
  - "every handle advertises exactly what it supports" — at this point I don't know what a
    "handle" is; the sentence only lands after chapter 4.
  - "Defined, loaded, and sealed at runtime" — "sealed" is doing undefined work. Sealed against
    what? Mutation? Further config?
  - The four-layer diagram labels `Table` as "transactional" and `Dio` as "live mode begins",
    but Vista sits between them with no mode label in the prose right above it — I had to
    re-read to confirm Vista is still transactional.
  - "Layers never leak upward: a `Table` doesn't know it's being cached" — that's downward
    ignorance described as upward non-leakage; the direction confused me on first read.
- **Paragraph value ratings:**
  - "Vantage is a data entity persistence…" — MEDIUM. One-liner, fine.
  - "Vantage changes the way you think…" — HIGH. Sets is the right first concept.
  - "Vantage offers two ways to work…" (transactional/live) — HIGH. Best paragraph on the
    page; I quoted it to my team.
  - "This documentation tracks the current 0.6…" — HIGH per word (it's one line), but it
    contradicts the site's 0.5.x.
  - Ethos bullets ("Let the backend do the work…" etc.) — HIGH. Six bullets, each a real
    design commitment I can test the book against. "Be aware of observers" is the weakest —
    it's a teaser, not a principle I can verify.
  - "Three ways to work with data" list — HIGH. Entity/Record/Rhai is the mental model the
    product page never gave me.
  - "The three interoperate: a Rhai-declared table…" — MEDIUM. Useful claim, never shown in
    the guide.
  - "Vantage is a framework, not a library…" — LOW. "takes over the data layer entirely",
    "all built on the same cohesive, extensible principles: what you learn in one crate
    applies in the next" — this is self-praise, not information. The 10+ crates count is the
    only datum.
  - "Vantage also doesn't mimic frameworks…" — MEDIUM. "No reflection, no runtime magic" is a
    real claim, but "feels native rather than translated" is performance prose.
  - "The four layers" block + following paragraph — HIGH. This is the page's payload. But it
    says **four** layers while the product site diagrams **nine** — reconcile them.
  - "Vantage and Vantage UI" — HIGH *for me specifically*; it's the paragraph that told me
    extending the framework can "carry further than the stock app does".
  - "Vantage covers a lot of ground — … none of that matters until you've seen it do
    something useful." — PADDING. Throat-clearing before the chapter list.
  - Chapter list (1–8) — HIGH. Dense, specific, each entry names its payoff.
  - "You'll need basic Rust experience…" — MEDIUM.
  - "Beyond the guide" list — HIGH. This is where my questions live; good that it's honest
    about that.
- **Engagement:** Engaged throughout — this is a strong opening page. Content is
  prose+diagram+lists, well balanced. I only skimmed the "framework, not a library" section.
- **Missing for me:** A one-paragraph map of the actual crates to the four layers (the site
  names nine things; the book names four; I want one picture). A sentence on stability policy
  across 0.x releases. And the intro should say up front that the *product* runs the
  Rhai/YAML path and this guide runs the typed path — I only worked that out on my own.

### step1-first-query.md

- **Unclear:**
  - `Column<T>` links to `vantage_table::column::core::Column`, but this chapter's Cargo.toml
    only added `vantage-sql`, `vantage-expressions`, `tokio` (later `vantage-types`,
    `vantage-core`). Where does `Column` come from — a prelude re-export of a crate I never
    added? As someone auditing the dependency tree, this matters and it's silent.
  - "Try `price.gt(10).eq("foobar")` — surprisingly, this compiles too. That's by design:
    type safety is enforced on the **first** operation" — I had to read this three times.
    You're telling me the type-safety guarantee decays after one hop and calling it design.
    Say *why* it's design (conditions are `AnySqliteType`-typed expressions) in one sentence
    up front, not as a surprise reveal.
  - "There is also a generic `ident()` that works when the backend type can be inferred" —
    the typed/generic ident split plus "backend-pinned wrapper" is a lot of machinery for a
    first chapter, and I couldn't tell when I'd actually be forced to choose.
  - "SQLite uses CBOR — a compact binary format" — CBOR as SQLite's *internal storage* of
    values inside Vantage, or on the wire? It reads like SQLite itself uses CBOR. Clarify
    it's Vantage's value-carrier for the SQLite driver.
- **Paragraph value ratings:**
  - "Vantage is a big framework. It covers…" — LOW. Restates the introduction's scope list
    almost verbatim ("SQL databases, SurrealDB, MongoDB, CSV files, REST APIs").
  - "What is a Query Builder?" admonish — MEDIUM. The four-language comparison is nice
    orientation, though anyone evaluating a Rust data framework knows what a query builder is.
  - "Goals for this chapter" — HIGH. Checkable contract; every chapter should have one (only
    steps 1 and 4 do — inconsistent).
  - "Set up" + dependency explanation — HIGH.
  - seed.sql block + "Run it" — HIGH. Concrete, reproducible.
  - "Start with an async main" + bullets — HIGH. `e.report()` rationale is exactly the level
    of detail I want.
  - "Connect to SQLite" + sqlx-pool admonish — HIGH, except "Vantage expressions will
    eliminate any need to execute queries directly" — a sales clause inside a tip; cut it.
  - ".context() — readable errors" admonish — HIGH. Real error output shown; good.
  - "Build a SELECT" — HIGH.
  - "Builder pattern" admonish — MEDIUM. The with_/add_ duality is useful; the closing "Use
    whichever fits your code" + "Same result" is filler.
  - "Execute it" + "When does Vantage hit the database?" — HIGH. The `.await` = network rule
    is a great debugging invariant.
  - "Adding conditions" + injection paragraph — HIGH.
  - "Types and persistence rendering" admonish — HIGH. bool→0 vs FALSE is exactly a
    2am-debugging fact.
  - "Typed columns and operators" — HIGH until the big admonish; the "Type safety and
    backend-specific operations" admonish is MEDIUM — important content (the decay of type
    safety, the `.clone()` ownership rule) buried in the longest block on the page.
  - "Primitives for untyped access" admonish — LOW *placement*. It introduces `sqlite_ident`,
    generic `ident`, `Fx`, `Case`, `Concat`, `Interval` — a reference-page dump the first
    chapter doesn't need; the final paragraph ("In Vantage, primitives, query builders,
    Column, Conditions and even native types like i64 and bool — all implement
    `Expressive<T>`") is actually HIGH and deserves promotion out of this admonish.
  - "Working with Any-types" — HIGH. `.try_get` returning `None` "no panics, no garbage" is
    the fail-loudly ethos made concrete.
  - "serde_json::Value conversion" warning — HIGH. Precision-loss warnings are gold.
  - "Under the hood, each persistence has its own type system…" — MEDIUM (see CBOR confusion
    above).
  - "Mapping rows to structs" + serde-alternative admonish — HIGH, but the serde admonish
    repeats the precision-loss warning from two sections earlier, nearly word for word
    ("`Decimal` precision, date types" / "decimals become floats").
  - "Putting it together" + full listing — HIGH.
  - "What we covered" table — HIGH. Keep these on every page.
  - "Going deeper" — MEDIUM. Good links; "None of it is required for the next chapter" is a
    kind touch.
- **Engagement:** Engaged. Code/prose/admonish balance is good, though the page leans hard on
  admonish boxes — eight of them; by the sixth I started treating them as skippable, which is
  a problem because two contain load-bearing semantics (type-safety decay, ownership/clone).
- **Missing for me:** The connection-pool failure story (pool exhaustion, timeouts —
  `SqliteDB` "wraps an sqlx connection pool" and that's all I get). What `Expression`
  flattening does when nesting goes wrong. Nothing here blocks me, though — as a foundation
  audit this chapter passes.

### step2-tables.md

- **Unclear:**
  - The teaser code `overdue.ref_customer().send_reminder().await;` — `ref_customer()`
    returns a table of customers, so `send_reminder()` is a set-level action? The page says
    "Don't worry about the exact syntax", but this is the *money shot* of the set concept and
    I can't tell if actions-on-sets is real API or pseudocode. It never reappears.
  - `let all = table.list().await?; // IndexMap<String, Product>` — where did the id type
    `String` come from when the SQLite id column is `INTEGER`? Then `table.get("pie")` looks
    up by… name? The seed data's ids are integers. This example took me the longest to
    un-confuse on the whole page, and I'm still not sure `get("pie")` matches anything.
  - "`insert_return_id()` for when you want the database to generate the ID" — but the row
    above inserts with a string id "muffin" into an INTEGER PRIMARY KEY table. Is Vantage
    coercing? Would this fail loudly?
  - "in Rust code refactoring cascade through entire codebase. Vantage model layer contains
    that pain" — ungrammatical and I genuinely can't parse the claim.
- **Paragraph value ratings:**
  - "A table is a structure representing…" + code — HIGH.
  - "Think of `Table<SqliteDB, Product>` as a `Vec<Product>`…" — HIGH. Best analogy in the
    book.
  - "If you've used an ORM before…" — HIGH. Positions against ActiveRecord expectations.
  - "Operations like counting, filtering, and updating…" — MEDIUM; "if the database supports
    it, of course" is the first capability hint and deserves more than a shrug.
  - "When you `.clone()` a table, you don't clone the data…" — HIGH.
  - First set-examples bullet list ("all user records except…") — MEDIUM. Five examples where
    three would do.
  - Second bullet list ("notify customers who have an unpaid invoice…") — LOW. A second
    five-item list of hypothetical sets immediately after the first; the two lists say the
    same thing ("sets compose") and neither is code.
  - "Put these together and even complex operations…" + two code blocks — HIGH concept, but
    see the `send_reminder` confusion above.
  - "Don't worry about the exact syntax…" — PADDING as written; it's an apology for showing
    unteachable code.
  - Query-vs-Table comparison table — HIGH. This table format is the book's best recurring
    device.
  - "Defining a Table" code — HIGH.
  - "Hiding the db argument" tip — MEDIUM. Fine, but it shows a pattern the tutorial then
    refuses to use, and step 3 introduces a *third* pattern (OnceLock db()) anyway.
  - "This defines the `product` table once. You can now generate queries…" — HIGH; the bridge
    back to chapter 1's types is exactly right.
  - "CRUD operations" block — HIGH modulo the id-type confusion above.
  - "Idempotent operations" tip — HIGH. Retryability is an ethos payoff, shown.
  - "Listing any table" (generics vs Vista) — HIGH; good forward reference, and "`Vista`
    replaced the older `AnyTable` carrier in 0.5" is honest versioning.
  - "Relationships" through "Sets, not joins" admonish — HIGH. The emitted-SQL admonish is
    the proof-of-no-leak moment for this layer; more of this.
  - "Computed fields with expressions" section — HIGH.
  - "Expressions compose" admonish — MEDIUM. Valuable, but it's a ~40-line nested worked
    example inside an admonish; it should be a real section or move to the reference.
    "modern SQL databases optimise and execute them efficiently — there's no extra round
    trip" — the round-trip claim is true, the "optimise efficiently" claim is hope; correlated
    subqueries per row are exactly what I'd profile.
  - "Extension traits" section — HIGH. The unwrap-is-safe-because-registered argument is a
    real idiom decision, explained.
  - "Where to put extension traits" tip — MEDIUM; last paragraph ("This is how Vantage scales
    to large codebases…") drifts into pitch.
  - "Custom methods on extension traits" admonish — MEDIUM. print_table is nice color;
    "The table is yours to extend" + speculative method list is filler.
  - "Persistence Abstraction" — "It is time for a pause and reflection." PADDING opener.
    The mermaid diagram — HIGH. The following two paragraphs — MEDIUM.
  - The seven-checkbox benefits list — LOW/PADDING, and the worst-edited block in the book:
    "Cuting boilerplate, making code readablle" (two typos in one line), "refactoring cascade
    through entire codebase", "everything TypeScript has and more", "the performance of C".
    This is a marketing slide pasted into a tutorial, aimed at Python/Java/TS refugees, not
    at anyone reading chapter 2 of the actual book. Cut or rewrite to three factual bullets.
  - "Going deeper" tip — HIGH.
  - "A sneak peek at what's next" tip — LOW. Six teaser bullets ("almost for free",
    "Super-efficient", "The patterns you've learned scale all the way up"), one typo
    ("challenges that make Vantage is equipped to deal with"), and half the bullets (UI
    adapters, reactive data) duplicate what chapters 5–8 of *this same guide* already cover.
- **Engagement:** Engaged through relationships and expressions; skimmed both motivational
  bullet-list stretches (top and bottom of the page). The page is bookended by its weakest
  material.
- **Missing for me:** Write-path failure modes — what does `insert` on a violated constraint
  return, what does the error chain look like? The id-type story (typed ids? always
  stringly?). And the first honest note that `unwrap()` on `get_ref_as` is a *panic* policy
  choice the framework is delegating to me.

### step3-axum-server.md

- **Unclear:**
  - "Chapter 2's `Category` carried a computed `title` field" — no it didn't; `title` was
    inside an optional "Expressions compose" admonish, presented as a could-do. The
    walk-back paragraph treats admonish content as canon; I went back to check whether I'd
    missed a step.
  - The `crud` generic bound `E: Entity<AnySqliteType>` — `Entity` was never shown taking a
    type parameter before (step 2 used `E: Entity` bare in `list_table`). One sentence on
    what `Entity<AnyType>` means would have saved me a re-read.
  - "Axum only lets a handler run the `Path` extractor **once** per request — after that,
    the URL params are considered consumed." — is that actually axum's rule, or a
    simplification of extractor ordering? Stated as fact with no citation; if it's wrong,
    the whole HashMap design justification wobbles.
  - The Cargo.toml in "Migrating to MongoDB" uses `path = "../vantage-core"` etc. — path
    dependencies to sibling directories. A reader following along cannot build this. Either
    the guide is untested against crates.io versions or this snippet leaked from the repo.
    For an adopter verifying "published on crates.io", this is the single most
    trust-damaging line in the book.
- **Paragraph value ratings:**
  - Opening curl examples — HIGH. Show-the-end-state-first works.
  - "A few things about the shape of this API" bullets — HIGH.
  - "The handler functions are each written **once**…" — MEDIUM; repeats what bullet 1 just
    said about the same generic list handler.
  - "The minimum Axum skeleton" — Entities paragraph — HIGH (the round-tripping rationale
    for dropping computed fields is a real design lesson), modulo the "carried a computed
    title" mismatch above.
  - Both entity files — HIGH.
  - "Two things to notice" — HIGH.
  - Server listing + "A few notes on what this is doing" — HIGH.
  - "Caching table definitions" section — HIGH content; the three-bullet diff explanation is
    the right teaching move. But as an evaluator I note the framework made me invent a
    caching idiom (OnceLock + clone-the-static) by hand for something every app needs —
    why isn't this in the framework?
  - "When the cached table needs narrowing" admonish — MEDIUM. Third repetition of
    "clones the definition, not the data" (step 2 said it, this page says it here and again
    in "Products of a category").
  - "Products of a category" — HIGH, though "Chapter 2 introduced this pattern" +
    re-explaining extension traits is a re-teach of something two pages old.
  - "A generic `crud` helper" intro + shape analysis — HIGH. "That's not how you scale a
    codebase." is a stock beat but earns it here.
  - The 50-line `crud` listing — MEDIUM. Necessary, but it's the densest wall in the guide:
    five nested closures, Arc cloning, turbofish `Json::<Vec<E>>`, a
    `WritableDataSet::<E>::delete(&f(db(), &params), &id)` UFCS call that gets zero
    explanation (why the UFCS form? ambiguity? say so). For "good enough to read" Rust, this
    is the hardest passage in the book.
  - "Why a HashMap for path params?" admonish — HIGH (modulo the once-per-request claim).
  - "What's inside crud(), briefly" admonish — MEDIUM; re-explains the Arc trick the listing
    commentary already covered.
  - "Error handling" — HIGH throughout. "The worst part isn't the 500 — it's what happens on
    the wire when axum's request task panics" is exactly the failure-mode writing I want
    everywhere else.
  - "Why not match on the error message?" admonish — HIGH… until the MongoDB section
    *reintroduces exactly that string-matching* ("`message.contains("no row found") ||
    message.contains("Document not found")`"). The page argues against its own later code
    with no acknowledgment. As a reviewer of foundations: this tells me `get`'s
    Result<Option> discipline isn't uniform across drivers — say that explicitly instead of
    quietly regressing.
  - "Logging with {:?} on the server side" admonish — HIGH.
  - "Pagination and search" — HIGH; the compose-with-nested-scope paragraph is a good
    invariant.
  - "What about ordering?" admonish — HIGH honesty ("takes a small extra layer of `From`
    conversions that would balloon this section" — an admitted API wart, good), though it
    quietly reveals `OrderBy`'s generic design leaks into user code.
  - "Validating pagination params" admonish — HIGH. DoS note is platform-lead catnip.
  - "Migrating to MongoDB" — the payoff section. Entities diff — HIGH. The `$in` dual-push
    admonish — HIGH (this is a real cross-type footgun and they document it). But the
    migration honesty cuts against the intro's pitch: the intro said "migrate the whole
    server from SQLite to MongoDB by editing only the model", yet main.rs's `crud` bounds,
    id handling, and error mapping all changed. The chapter is honest about it; the *framing*
    oversold it.
  - "Running it" + curl transcript — HIGH.
  - "Scaling up: CRUD as a one-liner" — HIGH until the last two paragraphs; "That is the
    'one description, many operations' principle from chapter 2's `Table` carried all the
    way to the wire, unbroken." — victory-lap prose, LOW.
- **Engagement:** The strongest chapter for me until step 5. I slowed down (in a bad way)
  only inside the `crud` listing. Content is monotone-code-heavy compared to later chapters —
  a request/response diagram of crud's two operation shapes would help.
- **Missing for me:** Transactions. A write API doing POST/PATCH/DELETE and the word
  "transaction" never appears in the entire guide — that's my biggest single content gap for
  a *persistence* framework. Also concurrent-write behavior of `patch`, and whether
  `insert_return_id` is atomic per driver.

### step4-vista.md

- **Unclear:**
  - "every driver ships a `VistaFactory` that can materialize a Vista from a declarative
    YAML spec, with Rhai scripts for the expressions YAML can't state" — this is the
    product's actual operating mode, dispatched in one sentence. What does the YAML look
    like? Even a 5-line inline sample would anchor it.
  - "Vista doesn't carry its own condition type. Instead, it delegates to the wrapped
    driver" — then two sections later `TableShell` appears in the summary table
    ("Per-driver executor that Vista delegates to") having never been mentioned in the body.
    A named component surfacing only in the recap table means the page's architecture
    picture is missing a box.
  - "Narrowing is also one-way: there is no API to remove a condition once added" —
    followed by "Search and order are the exception — they carry replace semantics". So the
    mutability model is: conditions accrete, search/order replace, clone to widen. That's
    three rules for one handle; a 3-row table would fix what took me two reads.
  - `add_condition_eq("category_id", 1.into())` — is eq the only condition Vista can
    express? gt/lt/in through the erased handle? Never said; the capability struct doesn't
    cover it either.
- **Paragraph value ratings:**
  - Opening recap ("Chapters 1–3 built a typed data layer…") — MEDIUM; second paragraph
    ("A CLI that lists 'any table'…") — HIGH.
  - "Why 'Vista'?" admonish — LOW. Eleven lines of landscape poetry ("Stand at a vantage
    point — the peak above your infrastructure — and the landscape arranges itself below").
    One sentence of it is charming; the full metaphor tour (which then repeats for Diorama
    in step 5 and Scenery in step 7) is the book performing its own naming instead of
    informing. Its only hard fact — "a table isn't one of ours" — is worth keeping.
  - "Goals for this chapter" — HIGH.
  - "What Vista actually is" + progression block — HIGH.
  - "Vista trades away compile-time knowledge…" — HIGH, including the YAML-path aside
    (which should be bigger, per above).
  - "CBOR, not JSON" admonish — HIGH. Convert-at-the-boundary is a clear rule.
  - "Wrapping a typed Table" — HIGH. "That's it." + "No extra mapping code." — stock
    phrases, but short.
  - "Reading schema" + flag table — HIGH. "Flags are open — drivers and consumers can add
    their own" — good extension point, one line, exactly right.
  - "Adding conditions" + "Conditions mutate the shell" admonish — HIGH content, needs the
    consolidation noted above.
  - "Narrowing by id" — HIGH.
  - "Search and ordering" + "Not every driver supports these" warning — HIGH. First live
    demo of the capability contract.
  - "Pagination" both halves — HIGH. "The token is **opaque** — its shape is driver-private
    … Just round-trip it." — crisp contract writing.
  - "When neither is available" admonish — HIGH; the CSV fallthrough is an honest edge.
  - "Traversing references" — HIGH; the contained-relations-first routing order is real
    semantics.
  - "Cross-backend references" — HIGH. "That is deliberately *not* a Vista's job — a Vista
    honestly describes one backend" is the best layer-boundary sentence in the guide.
  - "Capabilities — the explicit contract" + println block — HIGH, though a struct of eight
    booleans printed one per line is a lazy way to show it; a table mapping *driver* ×
    *capability* (SQL / Mongo / CSV / DynamoDB) would double the information.
  - "Calling unsupported methods is an error" warning — HIGH. Unsupported vs Unimplemented
    distinction is precisely a 2am fact. The UI-adapter branching example connects to the
    product visibly.
  - "Putting it together" print_vista — HIGH.
  - "What we covered" table — HIGH (but introduces TableShell, see above).
  - "What's next" — MEDIUM.
- **Engagement:** Fully engaged — this is the chapter I came for and it mostly delivers. All
  prose+code though; the one diagram this page needs (Vista → TableShell → Table → driver)
  is the one it doesn't have.
- **Missing for me:** The `VistaCapabilities`-is-a-fixed-struct vs column-flags-are-open-strings
  asymmetry: if I write a driver with a capability the struct doesn't name, where does it go?
  Write-path capability detail (can_update vs patch vs replace granularity). And this was the
  natural page to sketch what implementing a driver takes — even a trait-name list — before
  pointing at new-persistence.md.

### step5-dio-lens.md

- **Unclear:**
  - The table-name DSL: "The table *name* is doing a lot of work here" — the guide's own
    words. `restxml/Contents@continuation-token=NextContinuationToken:s3/GET /{Bucket}?list-type=2`
    gets one paragraph of decoding and no grammar, no reference link, no error story (what
    does a typo in this string produce at runtime?). This is the pattern I must copy to wrap
    internal REST APIs and it's presented as incantation. Biggest unclear in the book.
  - "Callbacks receive `&Dio` and clone it to hold across `.await` — a cheap `Arc` bump" —
    took a re-read; say "Dio is internally an Arc; cloning is refcounting" once, plainly.
  - "opens a cache table named after the master" — named *what* exactly? This matters the
    moment two Dios share a redb file (which step 8 then does, via a different mechanism).
  - The `sync` fn writes with `dio.cache().insert_values(...)` and the events section then
    says everything that changes data announces on the bus — but step 7 has to add
    `dio.notify_dataset_changed()` after the same call, revealing raw cache writes are
    *silent*. The rule "cache writes don't announce; use patched()/removed() or notify
    yourself" is real and important, and it's never stated on this page where the habit is
    formed.
- **Paragraph value ratings:**
  - "Chapter 4 gave you Vista… this chapter picks a data source where it hurts" — HIGH.
    Motivation by pain, with a concrete target.
  - Diorama's three-things list — HIGH.
  - "Why 'Diorama'?" admonish — LOW/PADDING. Second landscape poem ("The vista from the
    peak is magnificent — and far away. Sooner or later you want a piece of it close at
    hand: on your desk, under glass, alive."). The Lens/Dio/Scenery sentence at the end is
    the only content; the rest performs.
  - Table/Vista/Dio comparison table — HIGH. The single most useful artifact in the guide;
    the Lifecycle row quietly carries the ownership model.
  - "Caching" paragraph (page segments vs record store) — HIGH; honest about deferring.
  - First SVG (master/cache/pump) — HIGH. The µs/ms annotations earn their pixels.
  - "A **Dio** owns exactly the two blocks above… That's what the **Lens** is for" — HIGH.
    The policy-vs-mechanism split is the chapter's thesis, well put.
  - First Lens code block — HIGH.
  - "With the Dio in place, there are two ways to consume it…" — HIGH, though this paragraph
    plus the second SVG plus "The facade Vista" section plus the facade/Scenery table say
    "facade = proactive, scenery = reactive" four times in one screen. One diagram + one
    table would do.
  - Second SVG — MEDIUM (mostly repeats the paragraph above it).
  - "The facade Vista" section — HIGH content (capability *widening* with honesty preserved
    is the key idea), but see repetition note.
  - facade/Scenery table — HIGH.
  - "The project: a weather-station inventory" — HIGH; "listing it is slow, every listing
    request is paid again on every run" sets the measurable target.
  - "No AWS account required" admonish — HIGH.
  - Setup + "vantage-aws is the S3/DynamoDB/IAM driver — it signs (or deliberately doesn't
    sign) requests itself, so there's no AWS SDK in the tree" — HIGH; that no-SDK fact is a
    real dependency-audit datum.
  - "files.rs" entity + serde renames — HIGH ("`Size` is a `String` because that's what the
    XML carries — no silent coercion" — ethos, demonstrated).
  - Table-name decoding paragraphs — MEDIUM only because incomplete (see unclear).
  - "A first listing" + timing — HIGH. "This is the itch the rest of the chapter scratches."
    — stock beat, forgivable.
  - "The master Vista, and what it can't do" — HIGH. Capability printout motivating Diorama
    is the layers cooperating on the page.
  - "`can_fetch_next` is worth pausing on…" — HIGH. The durable-cursor property ("this
    cursor survives process restarts") and its "the generic contract doesn't promise" caveat
    is exactly the abstraction-leak honesty I audit for — the guide flags that the resume
    trick is S3-specific. Model paragraph.
  - Lens/sync code + bullets — HIGH.
  - "Read the first statement again: the initial token is *the last filename already
    cached*." — HIGH. The re-read instruction is earned here.
  - "Reads come from the cache" + who-talks-to-what paragraph — HIGH; the sync-is-plumbing
    vs consumer-uses-facade distinction is the layer boundary made operational.
  - First/second-run transcripts — HIGH. Real timings; 2.3s → 17ms is the product's cache
    promise, proven.
  - "Invalidating" — HIGH; "A resuming cache has one blind spot… files deleted from the
    bucket linger locally" is an actual failure mode, stated plainly. More like this.
  - "That's the whole CLI: two seconds once, milliseconds forever after…" — MEDIUM; summary
    sentence, slightly performy.
  - "The event bus" — HIGH density (six event variants, row-level helpers, one paragraph)
    — arguably too dense; this paragraph carries chapter 7's whole substrate.
  - "Poll or push" admonish — HIGH. ChangeEvent/handle_event is my push-integration
    extension point, named.
  - "Writes, on a read-only master" admonish — HIGH. `WriteFailed … never a silent drop` —
    good; but this is the only sentence about write failure in the whole book. What's *in*
    the event? Is the queue durable across restart? Retried? Nothing.
  - Callback summary table — HIGH.
  - "What we covered" — HIGH.
- **Engagement:** The best chapter in the book. Diagrams, tables, code, timing transcripts —
  the diversity everything else should copy. I re-read only for the DSL string and the
  Arc-clone idiom.
- **Missing for me:** Write-queue durability and retry policy; what happens when two
  processes open `cache.redb` (redb is single-writer — does the second `make_dio` error
  loudly?); cache size governance (nothing evicts, ever, until step 8's app-level trick);
  and the cache-writes-are-silent rule noted above.

### step6-augmentation.md

- **Unclear:**
  - `Detail::Fixed(Arc::new(augmenter))` yet `augment` still demands
    `Arc::new(VistaCatalog::new())` — "ours can stay empty". Why must I construct an empty
    catalog to not use it? Reads like an API seam showing; either explain the design or own
    the wart.
  - `Source::Column { from: "Key", to: Some("prefix") }` — the trick only works because "A
    full filename used as an S3 prefix matches exactly one object". What if the detail
    lookup matches zero rows, or two? The merge behavior on miss/multi is undefined here.
  - `.with_lazy_expression("contents", …)` — "`contents` is deliberately absent [from the
    merge]: it exists only inside the detail fetch … and is never cached". So merge is also
    the cache-admission filter. Clear once you see it, but the sentence carrying the rule is
    inside a bullet; I'd promote it.
  - "rows a facade read returns come back hydrated — any of them still missing its augment
    columns runs the detail fetch first" — and if the detail fetch *errors* (S3 500, file
    gone between listing and fetch)? Does `fetch_window` fail wholesale, return the cheap
    row, retry? Not a word. That's my top failure-mode question for this layer.
- **Paragraph value ratings:**
  - Opening ("Chapter 5's inventory knows every station file's name and size…") — HIGH.
  - "**Augmentation** is Diorama's answer…" + target output block — HIGH.
  - "We build on chapter 5's crate unchanged…" — HIGH; the copy-forward bookkeeping is
    appreciated.
  - CSV sample + "the two columns we're after are cheap *derivations*" — HIGH.
  - Entity + "Lazy expressions" intro ("Chapter 2's `with_expression` won't do it — those
    expressions lower *into the backend's query*") — HIGH. Clean contrast of pushdown vs
    client-side; this is the layer boundary drawn exactly where I wanted it.
  - The pipeline code + numbered walkthrough — HIGH. "One download feeds every derived
    column declared after it" — the ordering rule stated as an invariant.
  - "Lazy expressions from YAML" admonish — HIGH *because* it finally shows one line of the
    config-driven world (`row.contents.split("\n").len() - 1`) — the only Rhai in the guide.
  - "Why not just list this table?" — HIGH. "That's not a listing — it's a batch job
    wearing a listing's interface." Best sentence in the book; the anti-pattern is explained
    before the pattern.
  - "Wiring the augmentation" + four-questions bullets — HIGH (modulo the empty-catalog and
    zero/multi-match gaps).
  - "Reads hydrate" + SVG — HIGH.
  - "Not every read, though… The rows you ask for are the rows that pay" — HIGH; the
    bounded-read rule is crisp.
  - Event plumbing snippet — HIGH.
  - First/second-run transcripts + "Real data, and readable at a glance: station
    GM000001474 … still reporting (May 2026); its neighbour … went silent at the end of
    1991" — HIGH; the narrative detail makes the data legible instead of decorative.
  - "(`--invalidate` still clears everything, derived columns included — derived data is
    data.)" — HIGH; four words carrying a policy.
  - "What we covered" + next/deeper — HIGH.
- **Engagement:** Fully engaged; shortest chapter, nothing padded. This page is the pacing
  benchmark.
- **Missing for me:** Detail-fetch error semantics (above); cost control — nothing stops a
  `fetch_window(0, 10_000)` from firing 10k downloads (step 8's scheduler helps concurrency,
  not volume); and whether augmentation re-runs when the master row changes (step 7 later
  mentions "demoted for re-hydration" — that rule belongs here).

### step7-scenery.md

- **Unclear:**
  - `page_size(200_000)` — the fix is "set past the archive size" so one list call builds
    the whole spine. So the spine is ~122k `EnrichedRecord`s in memory? What's the memory
    footprint, and what happens at the site's promised "million-row tables" — is a bigger
    page_size still the answer, or do multi-page spines work? The workaround is explained;
    the scaling model it implies is not.
  - "re-point the scenery at the ordered index for the new variant … Sorting back reuses
    the already-built index — zero list calls." — indexes appear here for the first and
    only time. Built where, stored where, invalidated when?
  - "Conditions are the exception: `where_eq` defines what the view *is*, so it's set at
    open, not toggled on a live scenery." — but search *is* toggleable and also narrows
    rows. The philosophical distinction (identity vs refinement) is asserted, not argued;
    I can't predict which future API goes in which bucket.
  - "keeps the scenery's viewport on a **ten-row band around the cursor**" — the adapter
    overrides the visible-range viewport story the whole chapter told (step 1 of the loop
    said "these rows are on screen"). Band-vs-screen is a real policy difference; one
    sentence on why (bandwidth? ordering?) is missing.
  - "identical opens share one instance under the hood" — identical by what key? (Step 8
    then needs `.exclusive()` precisely because of this — the sharing key matters.)
- **Paragraph value ratings:**
  - Opening ("Chapter 6's CLI asks for one window…") — HIGH; requirements-first framing.
  - Three-scenery bullet list — HIGH.
  - "All three share one reactivity mechanism…" — HIGH; "a burst of changes costs one
    repaint, not one per change" is the generation-counter contract in one line — this is
    the machinery behind the site's "reactive" claim, delivered.
  - "Why 'Scenery'?" admonish — LOW. Third landscape poem ("pan, and the scene follows;
    wait, and the light changes in front of you"). "Limited, but dynamic — that is the whole
    design" is the keeper; the camera tour is padding by the third occurrence.
  - "A Scenery is not another handle to your data — it hands you no records to keep." —
    HIGH.
  - Dio/Scenery comparison table — HIGH.
  - Five-step consumer loop — HIGH; the numbered loop plus the SVG that mirrors it is the
    best-engineered explanation in the book.
  - SVG — HIGH.
  - "Back to the inventory" requirements bullets — MEDIUM; four bullets that restate the
    chapter opening's own list ("the window should follow… rows should repaint… should stay
    current") in different words.
  - "Measure chapter 6's ending against that…" — HIGH; the async-wait-vs-user-wait
    distinction is honest and precise.
  - "The Scenery implements exactly this:" + three bullets — MEDIUM; third statement of the
    same three requirements on one page (opening → requirements list → this). Once as
    requirements, once as delivery is enough.
  - ratatui/adapters paragraph + cold-start screenshot — HIGH; naming egui/Slint/GPUI/
    Cursive/Tauri matters to me (product parity).
  - "learn-6 starts as a copy…" + widening changes — HIGH; the 100→1000 max-keys arithmetic
    ("1,220 round-trips of mostly latency") is good operational writing.
  - "A Lens that serves a UI" code + three explanations — HIGH; `on_list_page` gets "needs
    the most context" and receives it — two-pass loading is finally named and the
    QueryDescriptor surface is shown. `notify_dataset_changed` line — HIGH but see step 5's
    silent-cache-write complaint.
  - "The table" + `.open()` bullets — HIGH.
  - Live re-sort paragraph ("A UI rarely commits to one order at open time…") — HIGH
    content, but dense; four mechanisms (set_sort, atomic swap, hydration restart, index
    reuse) in one paragraph.
  - "One thing probably caught your eye: `page_size(200_000)`?!" — HIGH for owning the
    weirdness; incomplete per the memory question above.
  - "The viewport" — HIGH. "It is the load-bearing call, because **the viewport drives
    hydration**." Good. "Recognize it? This is chapter 6's bounded read with the asking
    automated" — HIGH; the guide's habit of aliasing new concepts to old ones is its best
    pedagogy.
  - "The running total" + honest-coverage bullets — HIGH; "the number in the status bar is
    honest about coverage" ties back to the ethos.
  - RecordScenery parenthetical — MEDIUM; fine as a footnote.
  - "Open freely, drop when done" — HIGH; drop = demand withdrawal is the lifecycle rule
    step 8 depends on.
  - "Binding to a terminal" + builder walkthrough — HIGH; "everything before this line was
    framework, everything after is the ratatui binding" is a boundary I can hold onto.
  - "Running it" — **PADDING, confirmed.** "the whole system visible at once… the rows
    around the cursor sprout numbers… `…` becoming `14355  20260710`… the `augmented`
    counter and `total rows` sum tick upward… jump to `End`, and the last stations of the
    alphabet get their turn." This is a paragraph of applause. It contains one new fact
    (End-key behavior implies hydration follows any jump) drowned in performed wonder —
    and unlike step 5/6, there's no transcript or timing to anchor it; it *narrates* an
    experience instead of *showing* one. Either paste a real second-by-second capture (the
    chapter already has the style for it) or cut to two sentences.
  - "Notice what the application never wrote: a render loop, a fetch, an event match." —
    HIGH; this closing observation is the actual payoff and would land harder without the
    preceding applause.
  - "What we covered" + next — HIGH.
- **Engagement:** Engaged through the loop, the Lens, and the builder; skimmed the third
  requirements restatement and "Running it". Diagram/table/code balance is good.
- **Missing for me:** Memory model of the spine (above); what a `TableScenery` does when
  the Dio's cache is *behind* the master mid-sync (are indexes stable while
  notify_dataset_changed storms in every page?); error surfacing in the UI path — if a
  detail fetch fails, does the row show `…` forever? Nothing in the chapter shows a scenery
  in an error state.

### step8-axum-dio.md

- **Unclear:**
  - "A watch is not a polling loop — it is a Scenery wearing an HTTP connection." — good
    line, but the *server-side* framing left me unsure until "The adapter" section whether
    watch responses replay the event bus or diff scenery state. (It's the latter — "The
    stream diffs against what it already sent" — but that's four sections later.)
  - The augment scheduler is described entirely in prose bullets — no API, no code, no
    event visibility. How do I *observe* the queues at 2am (depth? starvation?)? Is there a
    metric/event, or do I correlate curl timestamps like the demo does?
  - "still counted in the demand union" (on `.exclusive()`) — "demand union" is used as if
    defined; demand was a chapter-7 notion ("withdraws its demand") but "union" across
    consumers is new vocabulary introduced mid-sentence.
  - `ContentsCache` — is this framework or tutorial code? It's presented with a struct
    definition and admission policy like a shipped component, but lives in learn-7. Took me
    a re-read of "the cache is opened by hand" to conclude it's app code. Also: `seen:
    Mutex<HashSet<String>>` grows forever — the tutorial's own anti-eviction example has an
    unbounded ledger, unremarked.
  - "no lock is held across a network await anywhere in the read path" — a strong
    concurrency claim I want to believe; it's asserted for the whole read path of a
    multi-crate stack with no pointer to how it's ensured (clippy lint? design rule? test?).
- **Paragraph value ratings:**
  - Opening ("Chapter 7's client of the Dio was a terminal…") — HIGH.
  - Watch-vs-WebSocket/SSE paragraph — HIGH; adopting the Kubernetes shape and saying so is
    orientation I can reuse with my team.
  - "Vantage ships adapters for API backends the way it ships them for UI toolkits…" —
    MEDIUM; the symmetry point is made again in "The adapter" section ("the server-side
    sibling of the dataset-ui-adapters crate") — same sentence, twice on one page.
  - "Worth pausing on what the frontend gets for free: … the responsiveness chapter 7
    built for the terminal, now delivered over a wire." — LOW; pre-applause for a result we
    haven't seen yet (the transcripts later *do* prove it — trust them and cut this).
  - "One flight per row" problem statement — HIGH. "The fetches were nobody's job to
    coordinate." Clean motivation.
  - Scheduler bullets — HIGH content (round-robin, dedup, withdrawal-keeps-paid-work,
    worker count) despite the observability gap above; "paid-for work is kept" is a real
    invariant.
  - "Nothing in the example code changes for this…" — HIGH; layering demonstrated (behavior
    moved down a layer, API stable).
  - `.exclusive()` paragraph — HIGH modulo "demand union".
  - `DioRouter` code + "columns double as demand" — HIGH; the with_column-as-demand-gate is
    subtle and correctly flagged.
  - Route/mode table — HIGH.
  - "The split embodies the demand philosophy…" + NDJSON sample — HIGH.
  - "The stream diffs against what it already sent — a generation bump that changed nothing
    on this page costs nothing on the wire." — HIGH; wire-efficiency contract in one line.
  - "And the scenery is *owned by the response stream*…" — HIGH; lifecycle-by-HTTP is the
    chapter's best idea.
  - "The server" + cache-by-hand + blocking on_start rationale — HIGH ("A server should
    answer its first request from a warm cache" — policy with reason).
  - "Concurrency needs no further code." paragraph — MEDIUM (claim without mechanism, per
    above).
  - "A cache that earns its keep" — HIGH; lazy admission is a genuinely interesting
    pattern and the "gigabytes" arithmetic justifies it. Needs one line saying "this is
    application code — the framework gives you open_table and you bring policy", which is
    actually a *selling point* for extensibility left implicit.
  - "Watching it work" — HIGH, all of it. The three curl transcripts (cold detail 1.8s →
    21ms; disjoint watches alternating B,A,B,A; same-page watches landing same-second) are
    the strongest proof passages in the guide — the scheduler's fairness made externally
    observable. This is how you convince a platform engineer.
  - "Run the first `GET` again afterwards… Current knowledge grew." — HIGH.
  - "A tiny React client" — HIGH; twenty lines, no client library, AbortController →
    scenery drop closes the loop.
  - "The whole climb" success block — LOW/PADDING. A full-paragraph recap of all eight
    chapters ("Eight chapters ago this book started with one SQL query…") that restates
    each chapter's summary line; the reader who got here lived it. The final sentence
    ("Every layer still speaks through the one below it.") and the four reference links are
    the only load-bearing parts — keep those, cut the tour.
- **Engagement:** High throughout the scheduler and transcripts; skimmed the two
  pre-applause paragraphs and the closing recap.
- **Missing for me:** Auth story (every endpoint is anonymous; even a sentence — "put your
  tower middleware here, Vantage doesn't do authn" — draws the boundary). Watch-stream
  limits and backpressure (slow client, thousands of tabs). Scheduler observability. And
  the guide ends without ever circling back to *writes* in live mode — the write queue
  introduced in step 5 is never exercised; "reconciles instead of clobbering" from the
  introduction remains an unproven claim at the end of the book.

## Cross-page issues

- **Repetition / redundancy:**
  - **The landscape-naming poems**: "Why 'Vista'?" (step 4), "Why 'Diorama'?" (step 5),
    "Why 'Scenery'?" (step 7) — three admonishes running the same peak/camera metaphor
    ("Stand at a vantage point — the peak above your infrastructure" / "The vista from the
    peak is magnificent — and far away" / "Point a camera at a vista and you never capture
    the whole of it"). One naming box in the introduction would serve all four names; by the
    third, it's a tic.
  - **"Clone the definition, not the data"**: step 2 "When you `.clone()` a table, you don't
    clone the data — you clone the _definition_"; step 3 "That clone is what chapter 2 meant
    by 'cloning a table clones the definition, not the data'"; step 3 again "The clone
    copies the shape (columns, conditions, relationships), not any rows." Three full
    statements; one plus a back-reference suffices.
  - **Facade = proactive / Scenery = reactive** is stated in step 5 four ways within one
    screen (prose paragraph, second SVG, "The facade Vista" section, facade/Scenery table),
    then re-established in step 7's opening loop.
  - **Chapter-6/7 requirement restatement**: step 7 states the instant-listing +
    hydrate-what-you-look-at requirement three times on its own page (opening paragraph,
    "Back to the inventory" bullets, "The Scenery implements exactly this" bullets), after
    step 6's "Why not just list this table?" already argued it.
  - **Precision-loss warning** (Decimal/dates via serde_json) appears twice in step 1 alone
    (the `into_record` warning and the "Serde alternative" admonish) and again in step 4's
    CBOR box.
  - **Adapter symmetry sentence** in step 8: "Vantage ships adapters for API backends the
    way it ships them for UI toolkits" and, three sections later, "the server-side sibling
    of the `dataset-ui-adapters` crate".
  - **Stock phrases overused**: "That's it." / "That's the whole X" (steps 2, 3, 4, 5, 8);
    "earn its keep / earn their keep" (step 3 "Three things earn their keep", step 8 "A
    cache that earns its keep"); "honest / honestly" (I counted 11 across the guide —
    the concept is good, the word is wearing out); "the natural choice/shape" ×4 in the
    introduction alone; "pay / paid for" as the cost metaphor saturates steps 5–8; "This is
    the itch the rest of the chapter scratches" / "Now we cash the check" — cute once each,
    and they are each used once, to be fair.
- **Pacing:**
  - Step 2 drags at both ends: two consecutive five-item hypothetical-set lists up top, and
    the checkbox-benefits + sneak-peek marketing double at the bottom. Its middle
    (relationships → expressions → extension traits) is well paced.
  - Step 3's `crud` listing is the guide's comprehension cliff — 50 lines of nested
    closures with the explanation *after*; interleave or pre-shape it.
  - Step 4 → step 5 is the guide's biggest jump: from a local SQLite Vista straight into
    the AWS wire-protocol table-name DSL, unsigned requests, and redb — three new
    subsystems on one page. The DSL needs either a paragraph more or a reference link.
  - Step 6 is the pacing benchmark: one concept, fully landed, no padding.
  - Steps 5–8 escalate well; step 8's scheduler-in-prose is fast (no code for the central
    new mechanism) while its transcripts are perfectly paced.
- **Continuity:**
  - Step 3 says "Chapter 2's `Category` carried a computed `title` field" — `title` only
    existed inside an optional admonish exercise in step 2. Canon vs aside is blurred.
  - Step 3's "Why not match on the error message?" admonish condemns exactly the
    `message.contains("no row found")` code the same page's MongoDB section then ships.
  - Introduction promises "migrate the whole server from SQLite to MongoDB by editing only
    the model"; step 3's migration also edits `crud`'s bounds, id handling, and error
    mapping in main.rs. The chapter is honest; the promise isn't.
  - `TableShell` debuts in step 4's summary table without body-text introduction; "demand
    union" debuts mid-sentence in step 8; "index" (the sort-variant index) debuts and
    vanishes within one step-7 paragraph.
  - Step 1 uses `Column` (linked to `vantage_table`) without `vantage-table` in the
    dependency list.
  - Step 3's Cargo.toml shows `path = "../vantage-core"` sibling-path dependencies — not
    followable, and at odds with the site's "published on crates.io".
  - **Site vs book**: /framework says **nine layers** and **0.5.x**; the book says **four
    layers** and **0.6**. Site headlines "million-row tables"; the book's biggest
    demonstration is 122k rows via a `page_size(200_000)` workaround with no memory story.
    Site's core workflow (AI agent writing YAML over MCP) corresponds to the
    Config-Driven-Vistas path, which the guide defers on every page it touches (steps 2, 4,
    5, 6, 8 all point at the same reference chapter) and never demonstrates beyond one
    inline Rhai expression in step 6. Site lists GraphQL and CLI tools among sources;
    the guide never mentions either.

## Top 10 fixes I'd make

1. Add a "Config path" interlude (or extend step 4): one YAML table spec + VistaFactory
   materialization + what a broken spec's error looks like — it's the path the product runs on.
2. Document the vantage-aws table-name DSL: grammar, all segments, failure behavior of a bad
   string, and a reference link — it's the template adopters will copy for their own APIs.
3. Fix step 3's Cargo.toml to crates.io versions and CI-test every chapter's code against
   published crates (the "docs4 learn crates" exist — wire them up and say so on page 1).
4. Reconcile site and book: nine layers vs four, 0.5.x vs 0.6, and state the
   million-row memory model where `page_size(200_000)` is introduced.
5. Rewrite step 7's "Running it" as a real timed transcript (the step 5/6/8 style) and cut
   the applause; same treatment for step 8's "Worth pausing on what the frontend gets for
   free" and "The whole climb" recap.
6. Delete step 2's checkbox-benefits list and "sneak peek" block (or reduce to three factual
   bullets); fix its typos ("Cuting", "readablle", "make Vantage is equipped").
7. Add a failure-modes admonish per layer: detail-fetch errors during hydration (step 6),
   write-queue durability/retry and WriteFailed contents (step 5), two processes on one
   redb file (step 5), watch backpressure/auth boundary (step 8).
8. Collapse the three naming poems into one introduction box; state the
   cache-writes-are-silent / notify_dataset_changed rule in step 5 where the habit forms.
9. Resolve step 3's self-contradiction: either make Mongo's `get` return `Result<Option>`
   like SQLite's or explicitly flag the driver inconsistency instead of quietly
   string-matching after arguing against it.
10. Exercise one live-mode write before the book ends — the write queue and "reconciles
    instead of clobbering" are promised in the introduction and never demonstrated.
