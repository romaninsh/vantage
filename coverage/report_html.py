"""HTML report of overlapping tests, for people. matrix.sh doesn't run it.

usage: python3 -I coverage/report_html.py coverage/report/<package> <crate_dir> [jaccard]

Renders the same analysis as report.py's markdown (overlap.py) into
<out_dir>/report.html: summary tiles, clusters with a shared/unique bar per
test, and each test's source.
"""
import html
import os
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from overlap import load  # noqa: E402

OUT, CRATE = sys.argv[1], sys.argv[2]
# The run dir is coverage/report/<package>, named by matrix.sh.
o = load(OUT, CRATE, os.path.basename(os.path.normpath(OUT)), float(sys.argv[3]) if len(sys.argv) > 3 else 0.9)
DEST = os.path.join(OUT, "report.html")


def bar(k, scale):
    u, total = o.unique[k], len(o.sets[k])
    shared = total - u
    w_shared = 100 * shared / scale
    w_unique = max(100 * u / scale, 4.0) if u else 0.0
    tip = f"{o.short(k)} — {total} regions: {shared} shared, {u} unique"
    return (f'<div class="bar" title="{html.escape(tip)}">'
            f'<span class="seg shared" style="width:{w_shared:.2f}%"></span>'
            f'<span class="seg unique" style="width:{w_unique:.2f}%"></span></div>'
            f'<span class="nums">{total} <em>· {u} unique</em></span>')


def verdict(k, anchor):
    u = o.unique[k]
    beyond = len(o.sets[k] - o.sets[anchor]) if k != anchor else None
    extra = "" if beyond is None else f'<div class="note">{beyond} beyond anchor</div>'
    if u == 0:
        return f'<span class="flag cand">candidate</span><div class="note">0 unique</div>{extra}'
    return f'<span class="flag keep">covers {u} alone</span>{extra}'


def member_row(k, anchor, scale):
    rel, note = o.relation(k, anchor)
    code = html.escape("\n".join(o.loc[k][3]))
    return f"""
    <tr>
      <td class="name"><code>{html.escape(o.short(k))}</code><div class="loc">{html.escape(o.where(k))}</div></td>
      <td><span class="rel {rel}">{rel}</span><div class="note">{html.escape(note)}</div></td>
      <td>{verdict(k, anchor)}</td>
      <td class="barcell">{bar(k, scale)}</td>
    </tr>
    <tr class="src"><td colspan="4"><details><summary>source</summary><pre><code>{code}</code></pre></details></td></tr>"""


cluster_html = []
for n, c in enumerate(o.clusters, 1):
    anchor = c[0]
    scale = max(len(o.sets[k]) for k in c)
    kind = o.kind(c)
    count = o.candidates(c)
    rows = "".join(member_row(k, anchor, scale) for k in c)
    cluster_html.append(f"""
  <section class="cluster">
    <h3>Cluster {n} <span class="kind {kind}">{kind}</span> <span class="count">{len(c)} tests · {count} candidate{"" if count == 1 else "s"}</span></h3>
    <table><thead><tr><th>Test</th><th>Relation</th><th>Candidate?</th><th>Regions (shared · unique)</th></tr></thead>
    <tbody>{rows}</tbody></table>
  </section>""")

loner_scale = max((len(o.sets[k]) for k in o.loners), default=1)
loner_rows = "".join(
    f'<tr><td class="name"><code>{html.escape(o.short(k))}</code>'
    f'<div class="loc">{html.escape(o.where(k))}</div></td>'
    f'<td class="barcell">{bar(k, loner_scale)}</td></tr>' for k in o.loners)

tiles = [
    ("Tests measured", o.matrix["measured"]),
    ("Internal (src/)", len(o.internal)),
    ("Blackbox (tests/)", len(o.sets)),
    ("Regions covered", f"{len(o.regions) - len(o.dropped):,}"),
    ("Overlap clusters", len(o.clusters)),
    ("Tests in clusters", len(o.clustered)),
    ("No unique region", sum(1 for k in o.keys if o.unique[k] == 0)),
]
tile_html = "".join(f'<div class="tile"><div class="v">{v}</div><div class="l">{html.escape(l)}</div></div>'
                    for l, v in tiles)

page = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{o.crate_name} — overlapping tests</title>
<style>
:root {{
  --surface: #fcfcfb; --surface-2: #f3f2ef; --border: #e2e1dc;
  --text-1: #0b0b0b; --text-2: #52514e; --text-3: #75746f;
  --unique: #2a78d6; --shared: #eb6834;
  --warn: #b07800; --crit: #d03b3b;
  color-scheme: light dark;
}}
@media (prefers-color-scheme: dark) {{
  :root {{
    --surface: #1a1a19; --surface-2: #232321; --border: #383835;
    --text-1: #f0efec; --text-2: #c3c2b7; --text-3: #9a998f;
    --unique: #3987e5; --shared: #d95926;
    --warn: #fab219; --crit: #e66767;
  }}
}}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--surface); color: var(--text-1);
  font: 14px/1.5 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif; }}
main {{ max-width: 1180px; margin: 0 auto; padding: 32px 24px 64px; }}
h1 {{ font-size: 22px; margin: 0 0 4px; }}
h2 {{ font-size: 16px; margin: 40px 0 8px; }}
h3 {{ font-size: 14px; margin: 0 0 10px; display: flex; gap: 10px; align-items: center; }}
p.lede {{ color: var(--text-2); margin: 0 0 24px; max-width: 760px; }}
.tiles {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; }}
.tile {{ background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 14px 16px; }}
.tile .v {{ font-size: 24px; font-weight: 600; font-variant-numeric: tabular-nums; }}
.tile .l {{ color: var(--text-2); font-size: 12px; }}
.legend {{ display: flex; gap: 18px; color: var(--text-2); font-size: 12px; margin: 16px 0 0; }}
.legend i {{ display: inline-block; width: 10px; height: 10px; border-radius: 3px; margin-right: 6px; vertical-align: -1px; }}
.cluster {{ border: 1px solid var(--border); border-radius: 10px; padding: 16px; margin: 14px 0; background: var(--surface); }}
table {{ width: 100%; border-collapse: collapse; }}
th {{ text-align: left; color: var(--text-3); font-weight: 500; font-size: 12px; padding: 4px 8px; border-bottom: 1px solid var(--border); }}
td {{ padding: 8px; vertical-align: top; border-top: 1px solid var(--border); }}
tr.src td {{ border-top: none; padding-top: 0; }}
td.name {{ width: 40%; }}
td.name code {{ font-size: 12.5px; word-break: break-all; }}
.flag {{ font-size: 11px; font-weight: 600; padding: 1px 8px; border-radius: 999px; border: 1px solid var(--border); white-space: nowrap; }}
.flag.cand {{ color: var(--crit); border-color: var(--crit); }}
.flag.keep {{ color: var(--text-2); }}
.loc, .note {{ color: var(--text-3); font-size: 12px; }}
td.barcell {{ width: 28%; white-space: nowrap; }}
.bar {{ display: inline-flex; gap: 2px; width: 70%; height: 10px; vertical-align: middle; }}
.seg {{ display: block; height: 100%; border-radius: 4px; min-width: 0; }}
.seg.shared {{ background: var(--shared); }}
.seg.unique {{ background: var(--unique); }}
.nums {{ margin-left: 8px; font-variant-numeric: tabular-nums; font-size: 12px; }}
.nums em {{ color: var(--text-3); font-style: normal; }}
.rel, .kind {{ font-size: 11px; font-weight: 600; padding: 1px 8px; border-radius: 999px; border: 1px solid var(--border); color: var(--text-2); }}
.rel.identical, .kind.identical {{ color: var(--crit); border-color: var(--crit); }}
.rel.subset, .kind.contained {{ color: var(--warn); border-color: var(--warn); }}
.count {{ color: var(--text-3); font-weight: 400; font-size: 12px; }}
details summary {{ cursor: pointer; color: var(--text-3); font-size: 12px; }}
pre {{ background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px; padding: 10px 12px; overflow-x: auto; font-size: 12px; line-height: 1.45; margin: 6px 0 0; }}
</style></head>
<body><main>
<h1>{o.crate_name}: overlapping tests</h1>
<p class="lede">Every test ran alone with its own coverage profile; a region is a code span in
<code>{o.crate_name}/src</code>. Blackbox tests are clustered when one's covered regions contain
another's, or they share ≥ {o.jaccard:.0%}. {len(o.internal)} internal tests (under <code>src/</code>) are
left out. Identical coverage is a candidate, not a verdict — compare what each test <em>asserts</em>.</p>
<div class="tiles">{tile_html}</div>
<div class="legend"><span><i style="background:var(--shared)"></i>regions other tests also cover</span>
<span><i style="background:var(--unique)"></i>regions only this test covers</span></div>
<h2>Clusters with deletion candidates first, then by overlap</h2>
{''.join(cluster_html)}
<h2>Tests with no unique region, outside any cluster</h2>
<p class="lede">Everything these cover is covered somewhere else, spread across several tests.</p>
<div class="cluster"><table><thead><tr><th>Test</th><th>Regions (shared · unique)</th></tr></thead>
<tbody>{loner_rows}</tbody></table></div>
</main></body></html>"""

open(DEST, "w", encoding="utf-8").write(page)
print(DEST)
