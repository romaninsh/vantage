# Intro cleanup plan — from the two persona reviews

Sources: `intro-feedback-facade-architect.md`, `intro-feedback-ui-adopter.md` (both read the
full intro, 2026-07-15). Both reviewers independently flagged the same top items — treat
agreement as priority. Verdicts: both would spike, neither would adopt yet.

## Phase 1 — factual bugs & contradictions (trust killers; fix first)

1. **step3: self-contradiction on error matching** (both reviewers, top-3 for both). The
   "Why not match on the error message?" admonish condemns `contains("no row found")`; the
   MongoDB migration section ships exactly that. Fix: make the Mongo path honest — either
   align the driver behavior story or explicitly flag "Mongo's `get` doesn't yet return
   `Result<Option>` like SQLite's; until it does, this is the workaround" in the admonish.
   Check what learn-3's code actually does today before rewriting.
2. **step8: transcripts contradict prose.** Chapter claims "learn-7 is learn-6" (122k files);
   every transcript shows GM prefix, `"total":1122`. That's true — learn-7 deliberately uses
   the GM prefix so the server pre-fetch is seconds. Say so where learn-7 is introduced
   ("back to the GM prefix — a server should boot in seconds; the full archive works the
   same way") and stop implying full-archive.
3. **step3: `path = "../vantage-core"` Cargo.toml** — unfollowable, contradicts crates.io
   story. Show version deps in the chapter listing.
4. **step3 treats a step2 admonish as canon** ("Chapter 2's `Category` carried a computed
   `title` field"). Rephrase to "chapter 2's *Expressions compose* callout showed how to
   add one".
5. **step2 id confusion**: `get("pie")` / insert `"muffin"` against `id INTEGER PRIMARY
   KEY`. Fix the example (numeric ids) or add the id-stringliness explanation both
   reviewers begged for (step3's `"category_id": "1"` output has the same problem).
6. **step1 broken promises**: goals list says COUNT/SUM (never delivered on the page) —
   drop the goal or add the one-liner; `sqlite_ident()` introduced as "one of several
   primitives" without prior mention; `Column<T>` linked to vantage-table which the setup
   never added — one sentence on prelude re-exports.
7. **step8 "chapter 7's demand gate"** — term never introduced in ch7. Name it in step 7's
   viewport/demand text OR rephrase step 8.
8. **step4 `TableShell`** debuts only in the recap table — add one body sentence where
   delegation is described.
9. **introduction "transactional" vs ACID** (architect re-read twice): one defusing sentence
   ("not database transactions — stateless request/response; BEGIN/COMMIT is the backend's
   business and not wrapped by Vantage today").
10. **Delete stale `docs4/src/intro/step6-scenery.md`** (1-line untracked leftover).
11. **Typos** (if their blocks survive Phase 2): "Cuting boilerplate", "readablle", "make
    Vantage is equipped", "to another team an API", "refactoring cascade through entire
    codebase".

## Phase 2 — padding cuts & de-repetition (editorial sweep, page by page)

1. **step7 "Running it"** (user + both reviewers: confirmed forced). Replace the
   wonder-prose with a real capture: the style of step 5/6/8 — timed facts, a
   finished-state status line. Keep the closing "Notice what the application never wrote"
   observation (both reviewers: that's the real payoff) — cut the applause around it.
2. **"The whole climb" (step8)** → keep final sentence + the four reference links, cut the
   eight-chapter recitation (it's the third full telling of the arc).
3. **step2 bottom**: delete/reduce the seven-checkbox "This separation gives you" slide and
   the "sneak peek" teaser block (3 factual bullets max); kill "It is time for a pause and
   reflection."; merge the two consecutive hypothetical-set lists at the top into one.
4. **Naming poems (Vista/Diorama/Scenery)** — ⚠ decision needed: reviewers say collapse to
   one; the Scenery one was authored deliberately this week. Options: (a) keep Scenery's,
   cut Vista/Diorama poems to one factual line each; (b) one shared naming box in the
   introduction. Lean (a).
5. **step7 triple list** (requirements bullets → "more responsive UI 1-2-3" → "Scenery
   implements exactly this") — ⚠ the middle+third lists are the deliberate new structure;
   the *older* "Back to the inventory" requirements bullets are now the redundant copy.
   Trim those four bullets to one sentence and keep the 1-2-3 + implements pair.
6. **step8 "Worth pausing on what the frontend gets for free"** — ⚠ deliberate recent
   addition; reviewer calls it pre-applause. Option: move the sentence *after* the
   "Watching it work" transcripts where it's earned, or compress to one clause.
7. **Dedup canonical tellings** (say once, link after):
   - clone-copies-definition: canonical in step 2; step 3's two retellings → back-references.
   - capability honesty: canonical in step 4 ("flags aren't suggestions — they're a
     contract"); trim the other four.
   - precision-loss/CBOR: once in step 1 (merge the two same-page tellings), keep step 4's
     CBOR box short.
   - facade-Vista in step 5: told 3× (paragraph, SVG, section) — cut the section's first
     half, keep table.
   - extension-trait rationale: full in step 2; step 3 gets one clause.
   - adapter-symmetry sentence in step 8: said twice, keep the "server-side sibling" one.
   - step1: 6 admonishes — move "Type safety and backend-specific ops" + "Primitives" bulk
     to the reference chapters, keep 3-4 line stubs (promote the `Expressive<T>` closing
     line into body text).
8. **Stock-phrase sweep** across all pages: "That's the whole X" (5×), "wearing a/an X"
   (keep step 6's original), "earns its keep" (2×), "honest(ly)" (~11× — keep for the
   capability contract, vary elsewhere), "the natural choice/shape" (4× in introduction).

## Phase 3 — clarity insertions (small, high-value; all answerable today)

1. **step5: table-name DSL** — add a mini-grammar block (`protocol/element[@cursor]:service/
   METHOD path?query`) + one sentence on failure mode (what a typo produces at runtime —
   verify what actually happens) + link to the vantage-aws reference. (Adopter's #1.)
2. **step5: cache-writes-are-silent rule** — state where the habit forms: `insert_values`
   doesn't announce; `patched`/`removed` write-and-announce; `notify_dataset_changed()`
   after bulk writes (step 7 currently springs this).
3. **step6/7: hydration failure semantics** — we HAVE the machinery: detail-fetch error →
   `RowStatus::LoadFailed` on the row (partial columns stay visible), stale-while-refetch on
   demotion, `RecordLoadFailed` on the bus. One admonish in step 6 ("What if the download
   fails?") answers both reviewers' top failure-mode question.
4. **step7: ten-row band** — one sentence: band-vs-screen is the *adapter's* policy
   (HYDRATE_BAND), why (each fetch is a multi-second download), and that it's centered on
   the cursor. (Or fold into the dynamic-viewport work if we do it.)
5. **step7: "identical opens share"** — name the sharing key (query + demand); plants the
   seed for step 8's `.exclusive()`.
6. **step8: define "demand union"** at first use; one sentence.
7. **step8: ContentsCache is application code** — "the framework gives `open_table`; you
   bring the policy" (turn the blur into an extensibility selling point). Note the `seen`
   ledger is unbounded (either fix in learn-7 or acknowledge).
8. **step6: own the empty-catalog wart** (`Arc::new(VistaCatalog::new())` with
   `Detail::Fixed`) — one honest parenthetical.
9. **step4: mutability rules table** (conditions accrete / search+order replace / clone to
   widen) — 3 rows.
10. **step8: watch reconnect honesty** — no resume token (unlike k8s `resourceVersion`);
    reconnect = fresh snapshot + new watch. One sentence.
11. **steps 3+8: auth boundary sentence** — "endpoints are anonymous; wrap the Router in
    your tower middleware — Vantage deliberately doesn't do authn."
12. **introduction**: defuse "Be aware of observers" teaser (one clause pointing at ch7);
    link "record mode" mention to the earlier definition.

## Phase 4 — bigger items (each needs a separate go/no-go)

1. **SQL-backed live example** — the facade architect's #1: a Dio + watch endpoint over a
   SQL database. Could be a step-8 section (swap the master: same Lens/sync over chapter
   2's products table) or a short step 9. Highest-value single addition.
2. **Config path demo** — the adopter's #1: one 10-line YAML table spec + factory
   materialization + what a broken spec's error looks like. Natural home: step 4 (which
   also needs its runnable scaffold — reviewers: only chapter with no project, no output).
3. **Step 4 runnable scaffold** (learn-4 is taken; would be renumbering or a small inline
   crate) with real print_vista output.
4. **Introduction additions**: backend support matrix (Oracle discoverable in 10s), license
   sentence, deployment topology sentence (single-process cache today; N replicas = N
   caches), "the product runs the YAML path; this guide walks the typed path" framing.
5. **Live-mode write exercised** — write queue + `WriteFailed` demo (learn-5 CLI could
   attempt a write against the read-only master and show the event).
6. **Site ↔ book reconciliation** (vantage-web2 repo): nine layers vs four, 0.5.x vs 0.6,
   million-row claim vs 122k demo + memory story.
7. **Split step 3** (Mongo migration into its own page) — pacing fix; renumbering cost.
8. **Scheduler observability** (framework): queue depth/starvation metrics or diagnostics
   surface — goes to `agent/todo/`, not the book.

## Suggested order

Phase 1 in one sitting (small edits, big trust wins) → Phase 2 page-by-page sweep
(introduction, step1, step2, step3 heavy; 5-8 light) → Phase 3 insertions → mdbook build +
re-read → decide Phase 4 items individually.
