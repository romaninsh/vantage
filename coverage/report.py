"""Agent-readable reports of overlapping tests from matrix.py's output.

usage: python3 -I report.py <out_dir> <crate_dir> [jaccard]

Writes into <out_dir>: index.md, clusters-NN.md (8 clusters each, with a
decision column) and internal.md (tests under src/). report_html.py renders
the same analysis as HTML for people.
"""
import os
import re
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from overlap import load  # noqa: E402

OUT, CRATE = sys.argv[1], sys.argv[2]
CHUNK = 8
o = load(OUT, CRATE, float(sys.argv[3]) if len(sys.argv) > 3 else 0.9)


def md(text):
    return text.replace("|", "\\|")


def cluster_md(n, c):
    anchor = c[0]
    exempt = set().union(*(o.sets[k] for k in c if k in o.exempt))
    out = [f"## Cluster {n} · {o.kind(c)} · {len(c)} tests\n",
           "| decision | test | location | relation | regions | unique | beyond anchor | beyond exempt |",
           "|---|---|---|---|---:|---:|---:|---:|"]
    for k in c:
        rel, _ = o.relation(k, anchor)
        beyond = "" if k == anchor else str(len(o.sets[k] - o.sets[anchor]))
        decision = "EXEMPT" if k in o.exempt else ""
        outside = "" if not exempt or k in o.exempt else str(len(o.sets[k] - exempt))
        out.append(f"| {decision} | `{md(o.short(k))}` | `{o.where(k)}` | {rel} | {len(o.sets[k])} | {o.unique[k]} | {beyond} | {outside} |")
    out.append("\nreason:\n")
    return "\n".join(out)


for stale in os.listdir(OUT):
    if re.fullmatch(r"clusters-\d+\.md", stale):
        os.remove(os.path.join(OUT, stale))

chunks = [o.clusters[i:i + CHUNK] for i in range(0, len(o.clusters), CHUNK)]
for i, chunk in enumerate(chunks, 1):
    first = (i - 1) * CHUNK + 1
    body = "\n".join(cluster_md(first + j, c) for j, c in enumerate(chunk))
    open(os.path.join(OUT, f"clusters-{i:02d}.md"), "w").write(
        f"# {o.crate_name} clusters {first}-{first + len(chunk) - 1}\n\n"
        "Fill each `decision` cell with KEEP or DELETE (exactly one KEEP per cluster) and the\n"
        "`reason:` line under each cluster. A row already marked EXEMPT has an approved exception:\n"
        "it stays. A row whose `beyond exempt` is 0 is covered by the EXEMPT rows: mark it DELETE.\n"
        "The rows left over need exactly one KEEP between them, or none if no rows are left over.\n"
        "`unique` counts regions no other blackbox test covers; `beyond anchor` counts regions this\n"
        "test covers that the cluster's first test doesn't; `beyond exempt` counts regions it covers\n"
        "that no EXEMPT row in the cluster does.\n"
        "Locations are first-last line, attributes and doc comments included.\n\n" + body)

# Internal tests: every #[cfg(test)] module and src/**/tests* file, with its range.
modules = []
for f, lines in sorted(o.texts.items()):
    rel = os.path.relpath(f, o.crate)
    if not rel.startswith("src/"):
        continue
    if "/tests/" in f"/{rel}" or rel.endswith("/tests.rs"):
        modules.append((rel, 1, len(lines), "whole file"))
        continue
    for i, line in enumerate(lines):
        if line.strip() != "#[cfg(test)]" or i + 1 >= len(lines):
            continue
        m = re.match(r"(pub(\(crate\))?\s+)?mod\s+(\w+)\s*([;{])", lines[i + 1].strip())
        if not m:
            continue
        if m.group(4) == ";":
            modules.append((rel, i + 1, i + 2, f"declares `mod {m.group(3)};`"))
            continue
        depth, end = 0, i + 1
        for j in range(i + 1, len(lines)):
            depth += lines[j].count("{") - lines[j].count("}")
            if depth <= 0 and "{" in "".join(lines[i + 1:j + 1]):
                end = j
                break
        modules.append((rel, i + 1, end + 1, f"inline `mod {m.group(3)}`"))

by_file = {}
for k in o.internal:
    by_file.setdefault(o.loc[k][0], []).append(k)
lines_out = [f"# {o.crate_name} internal tests\n",
             f"{len(o.internal)} tests live under `src/`. Every one is deleted, along with the module",
             "or file that holds it and any `mod tests;` declaration pointing at it.\n",
             "## Test modules and files\n",
             "| location | what |", "|---|---|"]
lines_out += [f"| `{f}:{a}-{b}` | {what} |" for f, a, b, what in modules]
lines_out.append("\n## Tests, by file\n")
for f in sorted(by_file):
    lines_out.append(f"### `{f}` · {len(by_file[f])} tests\n")
    lines_out += [f"- `{o.short(k)}` `{o.loc[k][1]}-{o.loc[k][2]}`"
                  for k in sorted(by_file[f], key=lambda k: o.loc[k][1])]
    lines_out.append("")
open(os.path.join(OUT, "internal.md"), "w").write("\n".join(lines_out))

index = [f"# {o.crate_name} test matrix\n",
         f"- tests measured: {o.matrix['measured']} ({len(o.internal)} internal under `src/`, {len(o.sets)} blackbox)",
         f"- failed or unmeasured: {len(o.matrix['failed'])}",
         f"- regions covered: {len(o.regions) - len(o.dropped):,}"
         f" ({len(o.dropped):,} more under paths left out: {', '.join(f'`{g}`' for g in o.excluded) or 'none'})",
         f"- overlap clusters among blackbox tests (≥ {o.jaccard:.0%} shared or contained): "
         f"{len(o.clusters)}, holding {len(o.clustered)} tests",
         f"- blackbox tests with no unique region outside any cluster: {len(o.loners)}\n",
         "## Files\n",
         "- `internal.md`: every test under `src/`, with module and file line ranges"]
index += [f"- `clusters-{i:02d}.md`: clusters {(i - 1) * CHUNK + 1}-{(i - 1) * CHUNK + len(ch)}"
          for i, ch in enumerate(chunks, 1)]
index += ["- `lost.txt`: regions lost against the baseline (when one exists)",
          f"\n## Exceptions (`coverage/exceptions/{o.crate_name}.yaml`): "
          f"{len(o.exempt)} accepted, {len(o.rejected)} rejected\n"]
index += [f"- EXEMPT `{o.short(k)}`: {e['reason']}" for k, e in sorted(o.exempt.items())]
index += [f"- REJECTED `{e.get('test')}`: {problem}" for e, problem in o.rejected]
index += ["\n## Blackbox tests with no unique region, outside any cluster\n"]
index += [f"- `{o.short(k)}` `{o.where(k)}` · {len(o.sets[k])} regions" for k in o.loners]
open(os.path.join(OUT, "index.md"), "w").write("\n".join(index) + "\n")

print(f"{len(o.internal)} internal tests, {len(o.clusters)} clusters ({len(chunks)} chunk files), "
      f"{len(o.clustered)} blackbox tests clustered, {len(o.loners)} loose no-unique -> {OUT}/index.md")
