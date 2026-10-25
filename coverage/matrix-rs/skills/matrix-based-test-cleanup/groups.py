"""Overlap groups for matrix-based-test-cleanup: tests of one crate whose region
sets are identical (at least FLOOR regions) or share at least NEAR, with each
test's source, from the last `coverage/matrix.sh <crate>` run.

usage: python3 -I coverage/matrix-rs/skills/matrix-based-test-cleanup/groups.py <crate>
writes coverage/report/<crate>/groups.md
"""
import json
import os
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
sys.path.insert(0, os.path.join(ROOT, "coverage"))
sys.dont_write_bytecode = True
from overlap import load  # noqa: E402

FLOOR, NEAR = 20, 0.98


def crate_dir(crate):
    meta = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"],
                                     cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    for p in meta["packages"]:
        if p["name"] == crate:
            return os.path.dirname(p["manifest_path"])
    sys.exit(f"no workspace crate named {crate}")


def jac(a, b):
    return len(a & b) / len(a | b) if a | b else 1.0


crate = sys.argv[1]
report = os.path.join(ROOT, "coverage", "report", crate)
directory = crate_dir(crate)
ns = load(report, directory, crate)
keys = [k for k in sorted(ns.sets) if len(ns.sets[k]) >= FLOOR]
parent = {k: k for k in keys}


def find(k):
    while parent[k] != k:
        parent[k] = parent[parent[k]]
        k = parent[k]
    return k


for i, a in enumerate(keys):
    for b in keys[i + 1:]:
        if ns.sets[a] == ns.sets[b] or jac(ns.sets[a], ns.sets[b]) >= NEAR:
            parent[find(a)] = find(b)
groups = {}
for k in keys:
    groups.setdefault(find(k), []).append(k)
groups = [sorted(g) for g in groups.values() if len(g) > 1]

rel_dir = os.path.relpath(directory, ROOT)
lines = [f"# {crate}: {len(groups)} overlap groups\n"]
for n, g in enumerate(groups, 1):
    same = all(ns.sets[k] == ns.sets[g[0]] for k in g)
    lines.append(f"## {n} · {'identical' if same else 'near'} · {len(g)} tests · {len(ns.sets[g[0]])} regions\n")
    for k in g:
        rel, a, b, src = ns.loc[k]
        exempt = " EXEMPT" if k in ns.exempt else ""
        lines.append(f"### {ns.short(k)}{exempt} — {rel_dir}/{rel}:{a}-{b}\n```rust\n" + "\n".join(src) + "\n```\n")
open(os.path.join(report, "groups.md"), "w").write("\n".join(lines))
print(f"{crate}: {len(groups)} groups, {sum(len(g) for g in groups)} tests -> {report}/groups.md")
