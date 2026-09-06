# App migration to Vantage 0.38 — working list

Every app is taken one at a time: migrate syntax, flatten `inventory/`, load it,
fix what it reports, tick it off here.

Roots came from the app's own recent-projects store,
`~/Library/Application Support/<channel>/recent.json`, across all five channels
(Vantage, Admin, Chtags, Dev, Nightly). Dead entries pruned; live ones surveyed
for the three things 0.38 changes.

## What "migrated" means

| Change | Marker to find | Fix |
|---|---|---|
| Scenery scripts read scope directly | `scenery(…"${x.value}"…)` | drop the quotes and braces: `x.value` |
| List `text:` / `label:` are templates | a bare expression under a list's `params:` | wrap: `'${ … }'` |
| Six slots deleted | `expressions:`, `expr:`, `lazy:`, `render:`, `unit: { rhai }`, reference `rhai:` | move the computation table-side (query-sourced `rhai:` block) or use a dotted implicit reference |

## vantage-ui-examples

Ten apps under `apps/`, plus `vantage-releases` at the repo root.

| App | `inventory/` to flatten | Syntax work | Notes |
|---|---|---|---|
| bakery | yes | none | 6 yaml, 2 pages — smallest |
| cardroom | — | — | **0 yaml files.** Empty shell: fill or delete |
| cashgpt | already flat | **8 list formats** in `dashboard`, `manager`, `apps` | 89 yaml, 19 pages — largest. Scenery reads already migrated |
| faker-demo | yes | **6 list formats** in `dashboard`, `framework-charts` | includes two `#{ text, color }` labels, which stay maps under a single hole |
| launch-control | yes | none | 30 yaml, 12 pages |
| periscope | yes | none | scenery reads already migrated |
| space | yes | none | 117 yaml, 54 pages — most pages |
| spacex | yes | none | 22 yaml. Overlaps `space`? decide whether both survive |
| vantage-github | yes | none | 19 yaml |
| vantage-leads | already flat | none | 6 yaml, lead-capture demo |
| vantage-releases | already flat | none | 25 yaml, at repo root not under `apps/` — move it in? |

Seven apps carry the `inventory/` level. Flattening moves its contents up one and
updates the recent-projects paths, the per-app README and any `.agents` skill
copies that name the path.

## vantage-ui/examples

| Example | State |
|---|---|
| surreal-bakery | migrated already (worker `!include`, language comments) |
| csv-dio | check: no yaml found by the page scan, may be datasource-only |
| sqlite-dio | same |
| kubernetes | **0 yaml files** — empty, delete or fill |

## Apps outside the examples repo

| Root | Size | Verdict to decide |
|---|---|---|
| `~/Vantage Examples/launch-control` | 30 yaml, 12 pages | Almost certainly an older copy of `apps/launch-control`. Diff, then delete the loser |
| `~/Vantage/librarian-3` | 23 yaml, 9 pages | Clean syntax, no dead slots. **Best candidate to move in** |
| `~/Documents/Vantage/librarian-2` | 15 yaml | Superseded by librarian-3? |
| `~/Documents/Vantage/librarian` | 0 yaml | Dead |
| `~/Documents/Vantage/playground`, `playground-2` | 14 yaml each | Scratch. Harvest anything good, then drop |
| `~/Documents/Vantage/my-app` | 0 yaml | Dead |
| `~/Work/vantage/optimising` | 13 yaml, 4 pages | Lives in the vantage repo. Perf scratch — keep where it is |
| `~/Work/spacelift-ui/vantage-admin` | 23 yaml, 7 pages | Separate product. Fix in place, do not move |
| `~/Work/chtags/admin` | 37 yaml, 9 pages | Private. Fix in place |
| `~/Vantage/chtags-admin` | 34 yaml | Older copy of the above? Reconcile |
| `~/Work/vantage-private-apps/golf-2` | 35 yaml, 9 pages | Private. Fix in place |

## Finding that needs a decision before release

**Deleting the six slots breaks three real apps.** The original survey found zero
YAML users, but it covered the examples repo only. Across the wider set:

| App | `expr:` | `lazy:` | `render:` |
|---|---|---|---|
| `vantage-private-apps/golf-2` | 23 | 1 | 2 |
| `chtags/admin` | 22 | 1 | 2 |
| `Vantage/chtags-admin` | ~22 | 1 | 2 |
| `spacelift-ui/vantage-admin` | — | — | 3 |

These will fail to load on 0.38. `expr:` is the bulk of it, and each one becomes
either a dotted implicit reference or a column projected by a query-sourced
table, so this is real per-column work rather than a rename. The deletion was
reconfirmed knowing four of the six were wired features, so this is not a reason
to reverse it — but it is a migration cost nobody has costed yet, and the private
apps are not in any CI that would catch it.

## Order of work

Smallest first, so the flatten-and-load procedure is proven before it meets the
117-file app.

1. **bakery** — flatten, load, fix. Establishes the procedure.
2. **vantage-leads** — already flat; load and fix only.
3. **vantage-github** — flatten, load, fix.
4. **spacex** — flatten, load, fix; decide whether it survives beside `space`.
5. **faker-demo** — flatten, plus the 6 list formats.
6. **launch-control** — flatten, load, fix; then diff against the `~/Vantage Examples` copy and delete the loser.
7. **periscope** — flatten, load, fix.
8. **cashgpt** — the 8 list formats, load, fix. Already flat.
9. **space** — flatten, load, fix. Largest, done last.
10. **vantage-releases** — decide whether it moves under `apps/`.
11. **cardroom**, **vantage-ui/examples/kubernetes** — fill or delete.
12. **librarian-3** — move into the examples repo if it earns a place.
13. Private apps (**chtags/admin**, **golf-2**, **spacelift-ui**) — migrate off the deleted slots.

## Per-app checklist

For each: flatten `inventory/` if present; migrate the three syntax changes;
open it on a 0.38 build; clear whatever `list_logs` reports; confirm each page
renders and its actions dispatch; update its README; re-point the recent-projects
entry.
