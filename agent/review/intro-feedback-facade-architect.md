# Intro review — facade-API architect persona

Reviewer context: I run a facade layer today — Oracle underneath, a pile of hand-written TypeScript
microservices on top, several frontends consuming it. Every new screen costs new endpoints; the
frontends full-refetch and feel dead. I'm evaluating Vantage as a declare-the-model,
get-the-API-and-reactivity-for-free replacement. Deep SQL, strong TS/Node, moderate Rust.

## Overall verdict

I would run a **two-week spike**, not a pilot — and the spike would exist *despite* chapters 1–4,
because of chapters 5–8.

**What convinced me.** Steps 5–8 demonstrate, end to end, exactly the property my system lacks:
step 3's "Adding a new entity to this server is now *two* things" is the low-code endpoint story;
step 8's watch endpoints ("a Scenery wearing an HTTP connection"), the augment scheduler's
round-robin/one-flight-per-row logs, and the React table whose "cells flip from `…` to numbers"
are the responsive-frontend story. The interleaved `[B] [A] [B] [A]` curl output in step 8 is the
single most persuasive artifact in the whole guide — real evidence, not adjectives. The
capability-honesty contract (step 4) is genuinely good engineering and I believe it.

**What scared me off.**
1. **The demo never touches my problem shape.** The reactive half (5–8) is built entirely over a
   public S3 bucket. The SQL half (1–3) is entirely transactional. The one thing I came for — a
   *live, cached, watchable view over a relational database* — is never built. It's addressed only
   in one admonish (step 5, "Poll or push — the Lens doesn't care") and I'm asked to extrapolate.
2. **Oracle is never mentioned.** SQLite, Postgres, MySQL, MongoDB, SurrealDB, CSV, REST, AWS —
   fine, but my data is in Oracle. "Adding a New Persistence — nine incremental steps" is the only
   hint at the cost, and the guide never sizes that effort.
3. **Rust burden.** Step 3's generic `crud` (Arc-wrapped closures, `Fn(SqliteDB, &Params) ->
   Table<SqliteDB, E> + Send + Sync + 'static`) is exactly the code my team of TS engineers would
   have to *own*. The guide says "one line per entity" but the 60-line generic machinery is mine
   to maintain and debug.
4. **Single-process cache.** The Dio's cache is a local `cache.redb` file. My facade runs N
   replicas behind a load balancer. Not one sentence in eight chapters addresses multiple server
   instances sharing or duplicating a Dio.

**Questions never answered anywhere:** Does Vantage support database transactions (BEGIN/COMMIT)?
The word "transactional" is used for something else entirely, which made this worse. What is the
license of the open framework (the closed Vantage UI is disclosed, the framework's terms aren't)?
How do I do authn/authz on these HTTP endpoints — there is an entire chapter on serving HTTP and
another on watch streams, and the word "auth" never appears? What happens when a watch connection
drops and reconnects — Kubernetes has `resourceVersion` resume semantics; this watch has nothing
stated? How mature is this — production users, API stability beyond "tracks the current 0.6"?
What's the memory/disk envelope of caching, and who evicts?

## Per-page feedback

### introduction.md

- **Unclear:**
  - "In **transactional** mode there is no state to manage" — as a SQL person I read
    "transactional" as ACID and had to re-read twice. It means *stateless request/response*. The
    collision is never defused, and actual DB transactions are never discussed in the whole guide.
  - "every handle advertises exactly what it supports" — meaningless until step 4; at this point I
    couldn't resolve what a "handle" is.
  - "Be aware of observers. Data knows who is watching" — poetic, unresolvable. Watching *what*,
    via what mechanism? (Answered four chapters later.)
  - "adding persistences and capabilities that custom builds can carry further than the stock app
    does" — I re-read this sentence three times and I'm still not sure what it's promising.
  - The four-layers block lists `Vista — record mode` but "record mode" was defined as
    `Record<V>` two sections earlier with no link between the two; I had to scroll back.

- **Paragraph value ratings:**
  - "Vantage is a data entity persistence…" — MEDIUM. Accurate but jargon-dense first line.
  - "Vantage changes the way you think…" — HIGH. The sets framing is the right opening move.
  - "Vantage offers two ways to work…" — HIGH content, marred by the "transactional" naming issue.
  - "This documentation tracks the current **0.6**" — MEDIUM. Useful, honest.
  - Ethos bullets — mostly HIGH ("Let the backend do the work", "Fill the gaps client-side" are
    the two best sentences on the page); "Be aware of observers" is LOW — it's a teaser wearing a
    principle's clothes.
  - "Vantage lets you choose how strongly typed…" (Three ways) — HIGH for me; the Rhai path is
    the low-code hook I came for.
  - "The three interoperate: a Rhai-declared table…" — MEDIUM.
  - "Vantage is a **framework, not a library**…" — MEDIUM; positioning I needed, though "all built
    on the same cohesive, extensible principles" is filler within it.
  - "Vantage also doesn't mimic frameworks from…" — LOW. Defensive marketing ("No reflection, no
    runtime magic") with no information I can act on — and step 5's stringly-typed table name
    (`"restxml/Contents@continuation-token=…"`) later reads to me as exactly the runtime magic
    this paragraph disclaims.
  - "Everything above maps onto four layers…" + code block + "A `Table` is where your model…" —
    HIGH. Best block on the page; I screenshot-ed this for my team.
  - "[Vantage UI] is a native admin console…" — MEDIUM. Important disclosure, but it raises the
    unanswered license question for the framework itself.
  - "Vantage covers a lot of ground — multiple databases…" — PADDING. "none of that matters until
    you've seen it do something useful" is throat-clearing before the roadmap.
  - "This guide introduces Vantage concepts one at a…" — HIGH. The one-sentence arc (CLI → HTTP →
    DB swap → cloud API → reactive → React) sold me on reading all eight.
  - Numbered chapter list — HIGH.
  - "You'll need basic Rust experience…" — MEDIUM. Undersells it; step 3 needs more than "basic".
  - "Beyond the guide" list — MEDIUM.

- **Engagement:** Prose-only until the four-layers code block; I skimmed the Ethos on first pass
  and only re-read it after step 5 made it concrete. Monotone start, strong middle (layers), tidy
  end. No diagram on the one page that most needs an architecture picture.

- **Missing for me:** A backend support matrix (is Oracle on the roadmap?); the license; one
  sentence on deployment topology (single process? cluster?); one sentence defusing
  "transactional ≠ transactions"; a "who is running this in production" signal.

### step1-first-query.md

- **Unclear:**
  - "Try `price.gt(10).eq("foobar")` — surprisingly, this compiles too. That's by design" — this
    admonish undermined my trust in the type-safety pitch on the very page making it, and the
    justification ("type safety is enforced on the **first** operation") reads like a rationalized
    limitation. Too deep, too early, net-negative.
  - "`sqlite_ident()` is one of several **primitives**" — first ever mention of `sqlite_ident`,
    introduced as though I'd already met it. I scrolled up looking for it. It isn't there.
  - Goals promise "5. Run aggregates (COUNT, SUM) with one method call" — the page never runs an
    aggregate. That lands in step 2 (`get_count_query`). Broken promise on the goals list.
  - "although Vantage expressions will eliminate any need to execute queries directly" — asserted,
    not shown; reads as a sales line inside a tip.
  - `Column<T>` is linked to `vantage_table::column::core::Column` but the setup only added
    `vantage-sql`, `vantage-expressions`, `tokio` ("Three dependencies"). Presumably the prelude
    re-exports it, but the page contradicts its own dependency story.

- **Paragraph value ratings:**
  - "Vantage is a big framework. It covers…" — PADDING. Re-states the introduction's scope
    paragraph; "We'll get to all of that." is throat-clearing.
  - "What is a Query Builder?" admonish — MEDIUM. The Knex line is a good anchor for a TS person;
    the other three examples are list-padding.
  - "For this chapter we'll use SQLite…" — MEDIUM.
  - "Goals for this chapter" — HIGH format (I want this on every page), minus the aggregate
    over-promise.
  - "Set up" + "Three dependencies…" — HIGH.
  - seed.sql + "You now have products.db…" — HIGH. Concrete, verifiable.
  - "Start with an async main" + bullets — HIGH; the `e.report()` explanation is exactly the right
    depth.
  - "Already have an sqlx pool?" — HIGH for me — brownfield interop is the first thing an
    enterprise migration asks.
  - "`.context()` — readable errors" — MEDIUM.
  - "Build a SELECT" + "Builder pattern" admonish — HIGH / MEDIUM (with_/add_ duality useful).
  - "Execute it" + "When does Vantage hit the database?" — HIGH / MEDIUM ("You always know when a
    database call happens because you typed `.await`" is a genuinely good line).
  - "Adding conditions" + injection explanation — HIGH.
  - "Types and persistence rendering" — MEDIUM.
  - "Typed columns and operators" — HIGH.
  - "Type safety and backend-specific operations" admonish — LOW. ~25 lines of blanket-impl
    trivia, an admitted type hole, and clone semantics — none needed on page 1; it derailed me.
  - "Multiple conditions combine with AND" — HIGH, appropriately brief.
  - "Primitives for untyped access" admonish — LOW. Introduces five concepts (`ident`, typed
    idents, `Fx`, `Case`, `Expressive<T>`) in a footnote, referencing a function never used above.
  - "Working with Any-types" main flow — HIGH.
  - "serde_json::Value conversion" warning — MEDIUM.
  - "Under the hood, each persistence has its own…" (CBOR paragraph) — LOW here. Wire-format
    internals on page 1; and the same precision-loss point is now made twice on this page.
  - "Mapping rows to structs" + macro — HIGH.
  - "The macro needs `vantage-core`…" — MEDIUM (honest wart, fine).
  - "Serde alternative" admonish — LOW. Third telling of "Decimal loses precision, dates become
    strings" on one page — verbatim redundancy: compare "you'll lose precision on `Decimal` and
    `chrono` types (dates become strings, decimals become floats)" with "`Decimal` values lose
    precision and dates become strings".
  - "Putting it together" + run output — HIGH.
  - "What we covered" table — HIGH; keep this pattern.
  - "Going deeper" — MEDIUM; "None of it is required for the next chapter" is a good pressure
    release.

- **Engagement:** The mainline is a well-paced code/prose alternation and I stayed with it. The
  admonishes are the problem: six of them, several 20+ lines, and the two type-system ones pulled
  me out of the tutorial into reference material. Content diversity is fine (code, SQL, shell,
  one table); the page just carries a second, hidden page inside its callouts.

- **Missing for me:** Connection pooling behavior (pool size, timeouts) for a server context; a
  Postgres variant of the connect line (the whole page is SQLite and I'll never deploy SQLite);
  prepared-statement/plan-cache behavior — I think in Oracle terms about repeated query shapes.

### step2-tables.md

- **Unclear:**
  - CRUD example ids: `table.get("pie")`, `table.insert(&"muffin".to_string(), …)` — the seed
    schema has `id INTEGER PRIMARY KEY` with values 1–7. Where does the string id "pie" come from?
    Does insert with id "muffin" even succeed against an INTEGER column? I stopped and re-read the
    seed file; still unresolved. Later JSON output in step 3 shows `"category_id": "1"` — string —
    so *something* stringifies ids, and it's never explained.
  - "Another typical operation is expressing one set as a condition of another then perform an
    action" — grammar breaks mid-sentence; had to re-read.
  - "in Rust code refactoring cascade through entire codebase. Vantage model layer contains that
    pain" — I genuinely could not parse this bullet on first read.
  - `products.with_condition(products.price().gt(200))` in the extension-trait payoff — but
    `with_condition` consumes `self` and `products` came from `ref_products()` (owned), so it
    works — yet two paragraphs earlier tables were things you `add_condition` to. The
    with_/add_/clone ownership rules are drip-fed and I kept second-guessing which applies.

- **Paragraph value ratings:**
  - Opening code + "Think of `Table<SqliteDB, Product>` as a `Vec<Product>`…" — HIGH. Best
    single analogy in the guide.
  - "If you've used an ORM before, you might…" — HIGH. Sets-not-records is the core idea and this
    is where it lands.
  - "Operations like counting, filtering, and updating happen…" — MEDIUM; restates the
    introduction's "let the backend do the work".
  - "When you `.clone()` a table, you don't…" + first bullet list — HIGH.
  - "Each of these is a set — defined by…" + second bullet list — MEDIUM. Two consecutive
    five-item lists of hypothetical sets is one list too many; I skimmed the second.
  - "Put these together and even complex operations…" + the two teaser code blocks — HIGH. The
    `overdue.ref_customer().send_reminder()` snippet is the pitch, compressed.
  - Query vs Table comparison table — HIGH.
  - "A [`Table`] is typically defined in its own file…" + code — HIGH.
  - "Hiding the db argument" tip — MEDIUM.
  - "This defines the `product` table once. You can…" — MEDIUM.
  - "CRUD operations" — HIGH concept, undermined by the id confusion above.
  - "Idempotent operations" tip — MEDIUM; re-states the introduction's "retry safely" ethos bullet.
  - "Listing any table" — MEDIUM; honest bridge to Vista, though "`Vista` replaced the older
    `AnyTable` carrier in 0.5" is churn-history I didn't need.
  - "Relationships" + `with_many` bullets — HIGH.
  - "Sets, not joins" admonish — HIGH. The best admonish in the guide: shows the emitted SQL,
    explains why two matched categories compose. This is the page earning my trust.
  - "Computed fields with expressions" through the updated product.rs — HIGH.
  - "A few things to note" bullets — HIGH.
  - "Expressions compose" admonish — MEDIUM. Useful, but long — and step 3 later treats its
    `title` field as mainline canon ("Chapter 2's `Category` carried a computed `title` field"),
    which punishes anyone who skipped an *optional callout*.
  - "Extension traits" opening — HIGH.
  - "The trait declares the vocabulary; the `impl`…" — LOW. "The bodies must live impl-side, too:
    methods like `get_ref_as` are inherent to `Table`…" is a Rust-language lecture that stalls the
    tutorial.
  - "The `unwrap()` is safe because…" — MEDIUM.
  - "Where to put extension traits" tip — MEDIUM; "This is how Vantage scales to large codebases"
    is assertion, but the file-layout advice is real.
  - "Custom methods on extension traits" admonish — MEDIUM; `print_table` output adds variety.
  - "It is time for a pause and reflection." — PADDING. Announces reflection instead of doing it.
  - Mermaid four-component diagram — HIGH. First diagram in the guide; more of these.
  - "The majority of your business code can work…" / "The model definitions focus on describing…"
    — MEDIUM.
  - "This separation gives you:" checklist — LOW→PADDING. This is a sales slide embedded in a
    tutorial: seven checked boxes pitching to Python, JS, PHP, TypeScript, Java, C# audiences in
    turn, with typos ("Cuting boilerplate, making code readablle", "refactoring cascade through
    entire codebase") that tell me nobody proof-read the marketing they pasted in. It performs
    enthusiasm rather than informing — the clearest "forced" block before step 7.
  - "Going deeper" — MEDIUM.
  - "A sneak peek at what's next" — MEDIUM for me — "Facades and middleware APIs, almost for
    free — wrap your model to expose a filtered or transformed view to another team an API" is
    *literally my project*, and the one bullet describing it has a grammar hole ("to another team
    an API"). Also over-promises: "Zero-cost cross-persistence traversal" is later (step 4)
    described as deliberately not the Vista's job and punted to a catalog.

- **Engagement:** High through Relationships and Sets-not-joins; dipped at the extension-trait
  Rust lecture; bottomed out at the checklist, which I skimmed with rising skepticism. Good
  diversity (code, table, mermaid, terminal output).

- **Missing for me:** How `with_condition(is_deleted.eq(false))` interacts with writes (does
  insert into the set auto-set the flag? Step's own "set invariants" pointer suggests yes — one
  sentence here would have answered it); anything about many-to-many; whether correlated-subquery
  computed fields are sane on a big warehouse table (the admonish's "modern SQL databases optimise
  and execute them efficiently" is a hand-wave I've been burned by on Oracle).

### step3-axum-server.md

- **Unclear:**
  - **Self-contradiction on error mapping.** The admonish "Why not match on the error message?"
    says an earlier draft "matched `e.to_string().contains("no row found")` … brittle, because it
    hard-codes a vantage-internal error string" — and then the MongoDB migration section, same
    page, ships exactly that: `if message.contains("no row found") ||
    message.contains("Document not found")`. So the brittle pattern the page disavows is the
    pattern the page teaches for Mongo. This is the single worst credibility hit in the guide.
  - "Chapter 2's `Category` carried a computed `title` field" — no, chapter 2's *optional
    admonish* did. I went back and checked.
  - "That migration is a change to the model file" — then the migration section changes
    Cargo.toml, both entities, the `CategoryTable` trait's id type, `crud`'s generic bounds, the
    connection call, id handling, and error mapping. "Nothing in the request path learned that
    storage moved" is defensible for *routes*, but "editing only the model" (the intro's phrasing)
    is an overclaim I noticed and resented.
  - `Entity<AnySqliteType>` appears in `crud`'s bounds — `Entity` with a type parameter was never
    introduced (step 2 used bare `E: Entity` in `list_table`).
  - The pagination example claims "Missing params fall back to the defaults (page 1, 50 per
    page) — and if neither is supplied we don't touch pagination at all" — two mutually exclusive
    readings of "missing" in one sentence; the code resolves it, the prose fights it.

- **Paragraph value ratings:**
  - Opening curl/JSON — HIGH. Show-then-tell done right.
  - "A few things about the shape of this API" — HIGH.
  - "The handler functions are each written **once**…" — HIGH; this is the low-code thesis stated
    plainly.
  - "The minimum Axum skeleton" + entity rework — HIGH content; the "walking away from" computed
    fields paragraph is MEDIUM but buries a real product limitation (computed fields vs writable
    APIs — see Missing).
  - Server code + "A few notes on what this is doing" — HIGH.
  - "Caching table definitions" + three-bullet explanation — MEDIUM. A full page of `OnceLock`
    mechanics; necessary Rust, but this is where my TS-team-retraining cost estimate started
    climbing.
  - "When the cached table needs narrowing" admonish — MEDIUM; third telling of
    clone-copies-the-definition.
  - "Products of a category" — HIGH; but "Chapter 2 introduced this pattern. The trait gives the
    relationship a typed, discoverable name…" — LOW, near-verbatim repeat of step 2's
    extension-trait rationale ("no turbofish, no string").
  - "A generic `crud` helper" framing paragraphs — HIGH ("That's not how you scale a codebase" —
    agreed).
  - The 55-line `crud` listing — HIGH value, HIGH toll. This is the hardest code in the guide and
    it arrives with minimal preparation for the closure/Arc gymnastics.
  - "That's the whole thing. Five handler bodies…" — MEDIUM.
  - Verb tables (×2) — HIGH; instantly legible.
  - "Why a HashMap for path params?" — MEDIUM; honest about the stringly-typed cost.
  - "What's inside crud(), briefly" — LOW. Rust closure/Arc trivia; belongs in a footnote of a
    footnote.
  - "Error handling" section — HIGH throughout; the empty-reply-on-panic detail ("the connection
    is dropped and the client sees an empty reply") is exactly the operational texture I want.
  - "Why not match on the error message?" — MEDIUM on its own; see contradiction above.
  - "Logging with {:?} on the server side" — MEDIUM.
  - "Pagination and search" — HIGH; the composition note ("the closure … narrows the table
    *before* `crud` applies its own pagination") is important and well put.
  - "What about ordering?" admonish — HIGH honesty, LOW comfort: "takes a small extra layer of
    `From` conversions that would balloon this section" tells me sorting a generic endpoint —
    table-stakes for any grid backend — is currently awkward at exactly the layer I'd build on.
  - "Validating pagination params" — MEDIUM; right instinct (DoS cap), delegated to me.
  - "Migrating to MongoDB" sections — HIGH as a demo; the diff really is small. The `$in`
    dual-push admonish — LOW for an intro (driver internals); "String _ids are also an option" —
    MEDIUM.
  - "Scaling up: CRUD as a one-liner" — HIGH; the 30-line final main.rs is the page's proof.
  - "Every route plays by the same rules… carried all the way to the wire, unbroken." — LOW; the
    victory-lap paragraph after the point was already made.

- **Engagement:** Highest-stakes page for me and mostly it held me. I slowed badly at the `crud`
  listing and the OnceLock section; I perked up at error handling and the migration. Diversity is
  good (curl transcripts, tables, code). It is also the longest page by far — see Pacing.

- **Missing for me:** Auth/middleware — not even a "wrap the Router in your usual tower layers"
  sentence; input validation beyond JSON well-formedness (my facade validates per-field; `Json<E>`
  accepts any well-typed body); OpenAPI/schema generation (my frontends consume generated
  clients); the computed-fields-vs-writes limitation deserves a pattern ("read entity vs write
  entity"), because my facade's whole value is enriched read models *plus* writes; N+1/latency
  characteristics of the per-request table build.

### step4-vista.md

- **Unclear:**
  - Where does this chapter's code *live*? Every other chapter scaffolds a crate (`learn-1`,
    `learn-4`…) and runs something. Step 4 has no `cargo new`, no run output, no stated host
    project — floating snippets against, presumably, chapter 2's app. I actually flipped back to
    check whether I'd missed a setup block.
  - "Goals … 4. Fetch paginated results with `fetch_page` and `fetch_next`" uses `list_values`,
    `get_some_value`, `SortDirection` in code before any of them is introduced or imported.
  - "every driver ships a `VistaFactory`" then "For MongoDB it would be `MongoVistaFactory`, for
    AWS it would be `AwsVistaFactory`" — but step 5 obtains one via `aws.vista_factory()`. Two API
    shapes for the same thing, unexplained.
  - "Narrowing is also one-way: there is no API to remove a condition once added" — then two
    sections later "Both are **replace semantics** — calling again drops the previous
    filter/order." The clone-to-widen rule plus the search/order exception took me two reads.

- **Paragraph value ratings:**
  - "Chapters 1–3 built a typed data layer…" — MEDIUM; fair bridge, mild recap.
  - "A CLI that lists 'any table from any…'" — HIGH; motivates Vista in one paragraph.
  - "Why 'Vista'?" admonish — PADDING. Ten lines of landscape/photography poetry ("Stand at a
    vantage point — the peak above your infrastructure — and the landscape arranges itself
    below"). Zero technical content; the first of three such poems in the guide.
  - "Goals for this chapter" — HIGH.
  - "What Vista actually is" — HIGH; "Think of the progression" code block — MEDIUM (repeats the
    intro's four-layer ladder, minus two rungs).
  - "Wrapping a typed `Table` — this chapter's path — is also not the only way…" — HIGH for me;
    the YAML/Rhai factory is my low-code path and this is its clearest mention. Still deferred.
  - "CBOR, not JSON" admonish — MEDIUM; third precision-fidelity telling of the guide.
  - "Wrapping a typed Table" + "That's it. The factory harvests…" — HIGH.
  - "Reading schema" + flags table — HIGH; the flags vocabulary ("id"/"title"/"searchable"…) is
    exactly what a generic frontend binds to. "Flags are open" — good.
  - "Adding conditions" + driver translation — HIGH.
  - "Conditions mutate the shell" admonish — HIGH; semantics that matter, well stated (modulo the
    replace-semantics wrinkle above).
  - "Narrowing by id" — MEDIUM.
  - "Search and ordering" — MEDIUM.
  - "Not every driver supports these" warning — MEDIUM; second statement of the honesty contract
    on this page.
  - "Pagination" (`fetch_page` / `fetch_next`, opaque token) — HIGH. Real API-design content; the
    DynamoDB/REST token examples ground it.
  - "When neither is available" — MEDIUM.
  - "Traversing references" + routing order — HIGH.
  - "Cross-backend references" — HIGH relevance, LOW satisfaction: "That is deliberately *not* a
    Vista's job" is a principled answer, but my facade is precisely a multi-source join surface
    and the actual mechanism (`VistaCatalog`) is a name plus a link.
  - "Capabilities — the explicit contract" + println block — HIGH; the CSV/SQL/DynamoDB tri-example
    makes it concrete.
  - "Calling unsupported methods is an error" warning — MEDIUM; *third* honesty-contract telling
    on the page, though "UI adapters branch on these flags … scrollbar vs 'load more'" is the one
    genuinely new sentence in it.
  - "Putting it together" `print_vista` — MEDIUM. Fine demo, but with no runnable project it
    prints nothing for me; low information density for 30 lines.
  - "What we covered" — HIGH.
  - "What's next" — MEDIUM.

- **Engagement:** This is the guide's lull. Concept-necessary, but it's the only chapter where
  nothing runs, nothing is measured, and no output appears — an API tour. I skimmed
  Search/ordering and Narrowing-by-id. Diversity suffers: code and prose only, no diagram, no
  terminal transcript, on the page introducing the most abstract concept so far.

- **Missing for me:** A runnable artifact (wrap chapter 2's table, print the schema, show it);
  the cross-backend traversal actually exercised; a paragraph on Vista over a *REST API* backend —
  my facade wraps upstream services, not just databases, and REST-as-backend is listed in step 1's
  opening and never seen again in the guide.

### step5-dio-lens.md

- **Unclear:**
  - The table name `"restxml/Contents@continuation-token=NextContinuationToken:s3/GET
    /{Bucket}?list-type=2"` — the page admits "The table *name* is doing a lot of work here" and
    explains it well, but nothing says what happens when I typo it. Runtime error? Silent empty
    listing? After the introduction's "no runtime magic" pledge, an un-checked protocol DSL inside
    a string is exactly where I wanted the failure mode spelled out.
  - "Page segments hold windows of an ordered query result … We keep it simple for now" — a
    forward reference to machinery I can't see, in the section explaining caching; I re-read it
    after chapter 7 and only then understood why it was mentioned.
  - `sync` uses `Instant::now()` and `CborValue` with no import trail; "the `CborValueExt` helpers
    the prelude brings in" — *which* prelude?
  - In the Lens snippet, `Lens::new().cache_at(…).on_start(…).build()?` — the first code block
    ends `.build()?` inside `Arc::new(…)` with a stray semicolon placement that made me unsure the
    snippet was complete; minor, but I paused.

- **Paragraph value ratings:**
  - "Chapter 4 gave you Vista — a universal…" — HIGH. Picks a data source "where it hurts" —
    exactly right.
  - "**Diorama** (`vantage-diorama`) is the layer…" + three numbered jobs — HIGH. Caching,
    capability injection, write routing — the crispest layer definition in the guide.
  - "Why 'Diorama'?" admonish — PADDING. Second naming poem ("on your desk, under glass, alive").
    The Lens/Dio/Scenery sentence inside it *does* carry the object model, but wrapped in ten
    lines of miniature-craft imagery.
  - Table/Vista/Dio comparison table — HIGH; the Lifecycle row is the most useful cell.
  - "Diorama caches at two levels…" — MEDIUM; forward-reference heavy (see Unclear).
  - First SVG (master/cache/pump) — HIGH. The µs/ms annotations do real work.
  - "A **Dio** owns exactly the two blocks above… That's what the **Lens** is for" — HIGH. The
    policy/mechanism split is the chapter's key idea and this paragraph nails it.
  - First Lens code block — HIGH.
  - "With the Dio in place, there are two ways to consume it…" — MEDIUM.
  - Second SVG (proactive vs reactive) — MEDIUM; attractive, but restates the paragraph above it.
  - "### The facade Vista" section — LOW. This is the *third* telling of the facade Vista on one
    page (paragraph, then SVG, then section), and phrases repeat almost verbatim: "the capability
    set can be *wider* than the master's, because the Lens decides what to add" vs "it carries the
    master's capabilities, and the Lens can manipulate and extend them". Cut one telling.
  - facade-vs-Scenery table — HIGH on its own; also the fourth artifact making the same contrast.
  - "The project: a weather-station inventory" — HIGH; the demo choice is inspired and the
    honesty ("listing it is slow… paid again on every run") sets up everything.
  - "No AWS account required" — MEDIUM.
  - `files.rs` code + "The table *name* is doing a lot of work" — HIGH explanation of a scary
    thing (see Unclear for what's still missing).
  - "The `@continuation-token=NextContinuationToken` part is **auto-pagination**" — HIGH.
  - "A first listing" + timing output — HIGH. "paid **on every run**, forever, because nothing
    remembers the answer" — good. "This is the itch the rest of the chapter scratches." — stock
    phrase, harmless here but part of a pattern.
  - "The master Vista, and what it can't do" + `fetch_next` cursor durability — HIGH. The
    cursor-survives-restarts insight is the best technical beat of the chapter.
  - "The Lens: pump pages, resume where you left off" + bullets + `sync` — HIGH. "Read the first
    statement again" earns its imperative.
  - "Reads come from the cache" + who-talks-to-what — HIGH; cold/warm run transcripts — HIGH.
  - "Invalidating" — HIGH, appropriately brief. "That's the whole CLI: two seconds once,
    milliseconds forever after" — fine summary, third "that's the whole X" of the guide.
  - "The event bus" — HIGH.
  - "Poll or push — the Lens doesn't care" admonish — HIGH, and for me the most important
    paragraph in the entire guide: SurrealDB live queries / Kafka / webhooks →
    `dio.handle_event(...)`. It is also **one admonish**. My core use case lives in this footnote.
  - "Writes, on a read-only master" — HIGH; `WriteFailed`, never a silent drop — consistent with
    the ethos, credited.
  - "Callback summary" table — MEDIUM; five of eight rows say "not used — chapter 7". It's a
    teaser table.
  - "What we covered" — HIGH.

- **Engagement:** The strongest sustained stretch so far. SVG diagrams, timing transcripts, real
  latency numbers — the diversity is exactly right. I skimmed only the facade-Vista third telling
  and the naming poem.

- **Missing for me:** The Postgres/Oracle version of this chapter, even as a sketch — what does a
  Lens over a SQL master look like? `refresh_every` against a 10M-row table — full re-list?
  Delta detection? Cache size management/eviction (the cache only ever grows); redb durability
  and corruption recovery; and the multi-instance question again (two servers, one bucket, two
  caches?).

### step6-augmentation.md

- **Unclear:**
  - `Source::Column { from: "Key", to: Some("prefix") }` — mapping a master column onto "the
    detail table's `prefix` *condition*" quietly relies on the S3 driver treating unknown
    conditions as query parameters (a step-5 fact). One recall-sentence would have saved me the
    flip-back.
  - "each callback *borrows the record as built so far*" then "Each callback clones what it needs
    out of the borrowed record before going async" — the closure-capture choreography (`move`,
    clones before `async move`) is visible in the code but never quite explained for a
    Rust-moderate reader; I trusted rather than understood it.

- **Paragraph value ratings:**
  - "Chapter 5's inventory knows every station file's…" — HIGH. "exactly the kind of expense you
    want to pay once and remember" — the whole chapter in a clause.
  - "**Augmentation** is Diorama's answer: enrich the master's…" + sample row — HIGH.
  - "We build on chapter 5's crate unchanged…" — MEDIUM; useful logistics.
  - "The detail side" + CSV sample + "So the two columns we're after are cheap *derivations*" —
    HIGH.
  - "Lazy expressions" intro ("Chapter 2's `with_expression` won't do it — those expressions
    lower *into the backend's query*") — HIGH. Precisely the right contrast.
  - The `Readings::table` listing — HIGH.
  - "Read it as a pipeline over one record" + numbered steps — HIGH; "One download feeds every
    derived column declared after it" is the takeaway, stated.
  - "Lazy expressions from YAML" admonish — HIGH for me; the only Rhai code in the whole guide
    (`row.contents.split("\n").len() - 1`). I wanted ten more lines of this.
  - "Why not just list this table?" — HIGH. "That's not a listing — it's a batch job wearing a
    listing's interface" is the best sentence in the guide; the whole section justifies the
    architecture instead of asserting it.
  - "Wiring the augmentation" + four-questions bullets — HIGH; `merge` excluding `contents`
    ("The megabytes stay out of the Dio; the two numbers stay in") — HIGH.
  - "Reads hydrate" + SVG — HIGH.
  - "Not every read, though… The rows you ask for are the rows that pay" — HIGH; the
    `list_values` vs bounded-read rule is crucial and clearly drawn.
  - Event plumbing snippet — HIGH.
  - Cold/warm transcripts + "Real data, and readable at a glance: station GM000001474 … is still
    reporting (May 2026); its neighbour … went silent at the end of 1991" — HIGH. Narration that
    *adds* information — contrast step 7's "Running it".
  - "(`--invalidate` still clears everything, derived columns included — derived data is data.)"
    — HIGH; parenthetical that answers the question I was forming.
  - "What we covered" / What's-next / Going-deeper — HIGH / MEDIUM / MEDIUM.

- **Engagement:** The best page in the guide. Nothing to skim; every block either shows code,
  shows output, or explains a decision. The 20.2s → 16ms pair of transcripts is the argument.

- **Missing for me:** Failure semantics — a lazy expression's download fails: is the row poisoned,
  retried, served un-augmented? (`WriteFailed` covered writes; hydration failure is unaddressed.)
  Staleness of derived columns when the source file changes (step 7 later mentions demotion — one
  sentence here would close the loop). Rate limiting the detail source — my upstreams throttle.

### step7-scenery.md

- **Unclear:**
  - "keeps the scenery's viewport on a **ten-row band around the cursor**" — is the band an
    adapter policy or a scenery default? The `.run()` bullet buries a load-bearing behavior in a
    parenthetical-grade clause.
  - "identical opens share one instance under the hood" — what counts as identical? Same sort +
    search + filter? (Step 8's `.exclusive()` implies the answer matters a lot.)
  - "Conditions are the exception: `where_eq` defines what the view *is*, so it's set at open,
    not toggled" — so a user-driven filter panel means reopening sceneries per filter change?
    That's a real UI-architecture consequence, dropped in half a sentence.
  - The status-bar sample shows "8000 rows · 2 augmented · total rows 15586" — "total rows" here
    means *sum of the `rows` column*, colliding with "row count" one bullet earlier. I parsed
    15586 as a row count first.

- **Paragraph value ratings:**
  - "Chapter 6's CLI asks for one window…" — HIGH; "What a UI actually needs is an ordered row
    set it can read *by index*…" — exact requirements, good.
  - Three scenery bullets — HIGH.
  - "All three share one reactivity mechanism…" — HIGH; "a burst of changes costs one repaint,
    not one per change" is the sentence my frontend team needs.
  - "Why 'Scenery'?" admonish — PADDING. Third naming poem ("pan, and the scene follows; wait,
    and the light changes in front of you"); "Limited, but dynamic — that is the whole design"
    performs profundity. By the third of these I skipped on sight.
  - "A Scenery is not another handle to your data…" — HIGH.
  - Dio vs Scenery table — HIGH (fourth comparison table of the guide; still earning its keep,
    but the format is now predictable).
  - "That last cell is where this chapter ends up…" + the 5-step loop — HIGH. The loop is the
    chapter's spine.
  - SVG — HIGH; numbered arrows matching the list is good craft.
  - "Back to the inventory" requirements bullets — HIGH.
  - "Measure chapter 6's ending against that…" — MEDIUM; fair.
  - "For a better experience we need a more responsive UI: 1. display results as soon…" — LOW.
    This numbered list restates the requirements bullets above it, and then "The Scenery
    implements exactly this:" introduces a *third* bullet list restating both. Three consecutive
    lists, one idea.
  - ratatui intro + adapter-crate mention + cold-start screen — HIGH; the `…` cells shown early
    pay off later.
  - "`learn-6` starts as a copy…" + prefix/max-keys changes — MEDIUM; the round-trip arithmetic
    ("1,220 round-trips of mostly latency") is slightly belabored.
  - "A Lens that serves a UI" code + the three explanations — HIGH; **`on_list_page`** ("An
    augmented Dio drives **two-pass loading**…") is dense but the most load-bearing paragraph in
    the chapter, and it delivers. "a cache that *is* the listing" — good.
  - "(The `println!` narration from chapter 5 is gone…)" — MEDIUM; nice continuity touch.
  - "The table" + `.open()` bullets — HIGH.
  - "A UI rarely commits to one order at open time…" (live re-sort) — HIGH; "the grid never
    blanks mid-reorder" answers a question every grid engineer has.
  - "One thing probably caught your eye: `page_size(200_000)`?!" — HIGH; anticipates the
    objection and resolves it.
  - "The viewport" — HIGH; "the load-bearing call" — deserved emphasis. "Recognize it? This is
    chapter 6's bounded read with the asking automated" — HIGH, genuine synthesis.
  - "The running total" + skipped-rows honesty — HIGH; "honest about coverage: it starts at zero
    and climbs" — good.
  - "(There is a third kind, **RecordScenery**…)" — MEDIUM.
  - "Open freely, drop when done" — MEDIUM; the drop-withdraws-demand rule matters, said plainly.
  - "Binding to a terminal" + builder + bullets — HIGH / MEDIUM (the `.run()` bullet is a
    paragraph wearing a bullet).
  - "## Running it" — PADDING, **confirmed forced**. Every fact in it already appeared: instant
    open (`on_start_blocking` section), rows streaming in (same), `…` → numbers (the screenshot
    and `.with_column` bullet), band follows cursor (viewport section), warm restart (chapter 5).
    What remains is performance-of-wonder vocabulary: "the whole system visible at once", "rows
    around the cursor sprout numbers", "the last stations of the alphabet get their turn". No
    command output, no numbers, no screenshot of the *finished* state — the one thing that would
    have made this section informative. Compare step 6, whose equivalent section is transcripts.
  - "Notice what the application never wrote: a render loop, a fetch, an event match." — MEDIUM;
    there's a real point here (the adapter contract), delivered as applause; keep the first
    sentence, cut the rest.
  - "What we covered" — HIGH; What's-next — MEDIUM (good bridge: "none of them should ever
    download a file another view already paid for").

- **Engagement:** Front-loaded prose — the first third (concept, poem, table, loop, SVG) is all
  explanation before any project code, and I felt it. Once the Lens code lands the page moves
  well. The ending deflates: the chapter's climax is narrated instead of shown.

- **Missing for me:** The finished-state screenshot; memory footprint of a 122k-row spine in a
  scenery; what `sort` on 122k cached rows costs and where the ordered index lives; how a scenery
  behaves when the Dio's cache is cold *and* the master is down.

### step8-axum-dio.md

- **Unclear:**
  - **The numbers contradict the prose.** "`learn-7` is learn-6 with the terminal swapped for the
    router" — learn-6 widened to the full archive (~122,000 files). But the server log shows
    "fetched 1000 files … fetched 122 files" (=1,122) and the first curl returns
    `"total":1122` with `GM…` keys — chapter 5's Germany prefix. Either the prefix was silently
    reverted or the transcript is from the wrong build; both readings cost me ten minutes.
  - "(chapter 7's demand gate, now per connection)" — chapter 7 never used the term "demand
    gate"; the phrase first exists in step 6's *Going deeper* pointer to a reference chapter. A
    back-reference to something never introduced.
  - `ContentsCache` is presented mid-chapter with a struct and one method signature
    (`get_or_fetch`) but no full listing — it's *application* code, yet it reads like framework;
    I couldn't tell where Vantage ends and the example begins until the second read.
  - "no lock is held across a network await anywhere in the read path" — asserted about framework
    internals with nothing I can verify; I noted it as marketing-grade assurance.
  - The React snippet elides the actual NDJSON framing ("…accumulate chunks, split on '\n'…") —
    the one fiddly part of the client is the part hand-waved.

- **Paragraph value ratings:**
  - "Chapter 7's client of the Dio was a terminal…" — HIGH.
  - "The traditional plumbing for live updates over HTTP…" — HIGH; the Kubernetes-watch framing
    is exactly right for an enterprise reader, and "A watch is not a polling loop — it is a
    Scenery wearing an HTTP connection" is a genuinely clarifying line (though the "wearing a/an
    X" construction is now a house tic — see cross-page).
  - "Vantage ships adapters for API backends the way…" — HIGH; "Worth pausing on what the
    frontend gets for free…" — borderline performative, but the three listed properties are
    concrete, so it earns its keep.
  - "## One flight per row" — HIGH, the best engineering section in the guide. The problem
    statement ("The fetches were nobody's job to coordinate") and all four scheduler bullets are
    dense with real semantics (round-robin, dedupe-with-shared-completion, drop-withdraws-work,
    worker count vs determinism).
  - "Nothing in the example code changes for this…" — HIGH.
  - "One builder flag is new… `.exclusive()`" — HIGH; the shared-vs-per-connection distinction is
    subtle and well motivated.
  - "## The adapter: `DioRouter`" + code — HIGH; "columns double as the watch sceneries'
    *demand*" is a big rule stated fast — the one sentence I'd expand.
  - Routes/modes table — HIGH.
  - "The split embodies the demand philosophy…" + NDJSON sample — HIGH.
  - "the stream diffs against what it already sent… A closed tab stops pulling — the same
    lifecycle rule as chapter 7's closing grid, now enforced by HTTP" — HIGH.
  - "## The server" + cache-by-hand + blocking `on_start` rationale — HIGH; the learn-6 contrast
    (UI wants non-blocking, server wants warm) is a nice symmetry.
  - "Concurrency needs no further code…" — MEDIUM; see Unclear.
  - "## A cache that earns its keep" — HIGH idea (lazy admission is a tasteful policy), MEDIUM
    execution: app-vs-framework boundary blurs, and the gigabytes-vs-repeat-traffic tradeoff
    paragraph is the right kind of reasoning.
  - "## Watching it work" — HIGH, all of it. The 1.8s-then-21ms detail pair, the ADDED/MODIFIED
    stream, and above all the two-watch interleaving and same-page fan-out transcripts — this is
    the proof section of the entire guide. "Current knowledge grew." — earned.
  - "## A tiny React client" — HIGH relevance, MEDIUM depth. Twenty lines for the payoff my
    frontend team came for; no reconnect, no error path, no full file. The chapter's thinnest
    section is my persona's most important.
  - "What we covered" — HIGH.
  - "The whole climb" success box — PADDING. A third full recitation of the eight-chapter arc
    (the introduction's list and each chapter's What's-next already told it), in triumphal
    register ("Every layer still speaks through the one below it"). The reference pointers at the
    end are the only functional lines; keep those, cut the parade.

- **Engagement:** High throughout — this page has the best evidence-to-assertion ratio after
  step 6. Curl transcripts with timestamps are the right medium. I only skimmed the closing box.

- **Missing for me:** Horizontal scaling (the elephant: N replicas, one redb each?); auth on
  these endpoints; watch reconnection/resume semantics; backpressure — 10,000 concurrent watch
  connections, one event bus, what breaks first; production hardening of `DioRouter` (timeouts,
  max connections); and the guide-closing gap — I still haven't seen this stack over a SQL
  database, which is where my facade would run it.

## Cross-page issues

- **Repetition / redundancy:**
  - **Three naming poems.** "Why 'Vista'?" (step 4: "the landscape arranges itself below"),
    "Why 'Diorama'?" (step 5: "on your desk, under glass, alive"), "Why 'Scenery'?" (step 7:
    "pan, and the scene follows; wait, and the light changes"). One metaphor pass in the
    introduction would cover all three; by the third I skipped on sight.
  - **Clone-copies-the-definition, told four times.** Step 2: "you don't clone the data — you
    clone the _definition_"; step 3: "That clone is what chapter 2 meant by 'cloning a table
    clones the definition, not the data'"; step 3 again: "The clone copies the shape (columns,
    conditions, relationships), not any rows"; plus step 2's comparison-table row.
  - **The capability-honesty contract, told five times.** Introduction ethos ("every handle
    advertises exactly what it supports"); step 4 warning ("it's better to fail clearly than to
    silently return unfiltered results"); step 4 capabilities ("The capability flags aren't
    suggestions — they're a contract"); step 5 ("The honesty contract still holds; the facade
    just advertises what the *pipeline* can do").
  - **Precision-loss / CBOR-vs-JSON, told four times.** Step 1 warning ("you'll lose precision on
    `Decimal` and `chrono` types"); step 1 serde-alternative admonish ("`Decimal` values lose
    precision and dates become strings"); step 1 under-the-hood CBOR paragraph; step 4 CBOR
    admonish ("CBOR preserves type fidelity that JSON loses").
  - **Extension-trait rationale, twice nearly verbatim.** Step 2: "no turbofish, no string, no
    `?`, and the compiler catches typos"; step 3: "so call sites stop carrying the `"products"`
    string and the `::<Product>` turbofish around."
  - **Facade Vista, three times within step 5** (paragraph, second SVG, dedicated section) —
    quoted in that page's ratings.
  - **Step 7's triple list** — requirements bullets, "we need a more responsive UI: 1–3", "The
    Scenery implements exactly this:" bullets — one idea, three lists in a row.
  - **The eight-chapter arc, told three times** — introduction's Getting Started list, the
    chained What's-next boxes, and step 8's "The whole climb".
  - **Stock phrases:** "That's the whole X" (step 3 "That's the whole thing" / "That's the whole
    surface", step 5 "That's the whole CLI", step 7 "that is the whole design" / "That's the
    whole contract between data and display"); "wearing a/an X" (step 6 "a batch job wearing a
    listing's interface" — great once; step 8 "a Scenery wearing an HTTP connection" — the mold
    is showing); "earns its keep" (step 3 "Three things earn their keep", step 8 section title "A
    cache that earns its keep"); "honest/honestly" as an all-purpose virtue word (ethos, step 4,
    step 5 "honesty contract", step 6 "the honest fix", step 7 "honest about coverage") — six-plus
    uses dulls a genuinely good concept; "the natural choice/shape" ×4 in the introduction alone.

- **Pacing:**
  - Step 1 drags in its callouts: six admonishes, two of them 20+ line type-system digressions,
    on the "something familiar" page.
  - Step 3 is roughly double the length of any other chapter and carries the hardest Rust (the
    generic `crud`); it's also where a Rust-moderate reader will stall. Splitting the Mongo
    migration into its own step would even the load.
  - Step 4 is the flat stretch — no project, no run, no output — between two chapters that both
    end in timed transcripts.
  - Steps 5–6 are the model: tight problem → code → measured result loops.
  - Step 7 front-loads ~40% conceptual prose before any project code, then ends on narration
    instead of output.
  - The jump *out* of the bakery is abrupt: three chapters of SQL/HTTP investment, and the
    database story simply stops at step 3 — the reactive machinery never returns to it.

- **Continuity:**
  - Step 3 relies on step 2's *optional admonish* as canon: "Chapter 2's `Category` carried a
    computed `title` field" — mainline step 2 never added `title`.
  - Step 8 back-references "chapter 7's demand gate" — a term chapter 7 never uses (it first
    appears in step 6's pointer to the Augmentation reference).
  - Step 8's transcripts (`"total":1122`, GM keys, 1,122-file sync log) contradict its own claim
    that learn-7 is learn-6 (122,000 files, full-archive prefix).
  - Step 1's "Primitives" admonish introduces `sqlite_ident()` as if previously seen ("is one of
    several primitives") — it never appeared before.
  - Step 2 uses string ids ("pie", "muffin") against the INTEGER PRIMARY KEY schema from step 1;
    id stringliness resurfaces unexplained in step 3's JSON (`"category_id": "1"`).
  - Naming drift: `with_search` (step 2) vs `add_search` (steps 3–4, with different
    replace-vs-add semantics implied); `SqliteVistaFactory::new(db).from_table(…)` (step 4) vs
    `aws.vista_factory().from_table(…)` (step 5).
  - Step 4 has no project scaffold while every other chapter names its crate (learn-1 … learn-7).
  - Introduction's "No reflection, no runtime magic" sits uneasily against step 5's
    protocol-in-a-string table names and the guide's pervasive stringly-typed relationship/column
    names (mitigated by extension traits, but the pledge oversells).

## Top 10 fixes I'd make

1. Add the missing bridge chapter (or a section in step 8): a Dio + watch endpoint over a **SQL
   database** — the facade-over-enterprise-data case the whole guide implies but never builds.
2. Fix step 3's self-contradiction: the Mongo migration reintroduces
   `message.contains("no row found")` string-matching on the same page whose admonish calls that
   pattern brittle.
3. Fix step 8's transcript/prose mismatch (`"total":1122` and GM keys vs "learn-7 is learn-6"
   with 122,000 files) — state which prefix learn-7 actually uses.
4. Introduction: add a backend support matrix (Oracle's absence must be discoverable in 10
   seconds), the framework's license, and one sentence defusing "transactional mode" vs ACID
   transactions — then say somewhere whether DB transactions exist at all.
5. Cut the three "Why 'X'?" naming poems to one line each (or one intro sidebar for all three).
6. Delete step 2's "This separation gives you:" checklist (typos included) and replace step 7's
   "Running it" narration with an actual finished-state screenshot/transcript.
7. Give step 4 a runnable scaffold (learn-3b or similar) with real output, like every other
   chapter — it's currently the only chapter where nothing runs.
8. Deduplicate: facade Vista told 3× in step 5, clone-definition told 4×, capability-honesty told
   5×, precision-loss told 4× — one canonical telling each, then link.
9. Add an auth/hardening paragraph to steps 3 and 8 (even just "wrap the Router in tower
   middleware; watch endpoints need X") and a watch reconnect/resume note — HTTP chapters with
   zero auth words won't survive an enterprise architecture review.
10. Show one complete YAML+Rhai Vista in the intro guide — the low-code path is promised in the
    introduction, step 2's sneak peek, step 4, and step 6's admonish, and demonstrated in exactly
    one Rhai expression fragment.
