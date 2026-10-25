"""Per-test coverage matrix for one crate. Run through coverage/matrix.sh.

usage: python3 -I matrix.py <out_dir>

Reads <out_dir>/covered.json, the code regions each test covers inside the
crate's src/ (written by coverage/matrix-rs, which runs every test alone with
its own LLVM profile), and reports which tests cover nothing no other test
covers.
"""
import json
import os
import sys
from collections import defaultdict

OUT = os.path.abspath(sys.argv[1])
data = json.load(open(os.path.join(OUT, "covered.json")))
tests, internal = range(data["tests"]), data["internal"]
covered = {k: {tuple(r) for r in regions} for k, regions in data["covered"].items()}
failed = [tuple(f) for f in data["failed"]]

hits = defaultdict(int)
for regions in covered.values():
    for r in regions:
        hits[r] += 1

rows = []
for key, regions in covered.items():
    unique = [r for r in regions if hits[r] == 1]
    rows.append((key, len(regions), len(unique)))

# Tests whose region set another single test fully contains.
subsumed = {}
items = sorted(covered.items(), key=lambda kv: len(kv[1]))
for i, (key, regions) in enumerate(items):
    if not regions:
        continue
    for other, oregions in items[i + 1:]:
        if regions <= oregions:
            subsumed[key] = other
            break

# Greedy minimal set cover keeping today's covered regions.
universe = set(hits)
remaining, keep = set(universe), []
pool_left = dict(covered)
while remaining:
    best = max(pool_left, key=lambda k, rem=remaining: len(pool_left[k] & rem))
    gain = pool_left[best] & remaining
    if not gain:
        break
    keep.append(best)
    remaining -= gain
    del pool_left[best]

redundant = sorted(k for k, n, u in rows if u == 0)
index = {r: i for i, r in enumerate(sorted(universe))}
json.dump({
    "regions": [list(r) for r in sorted(universe)],
    "tests": {k: sorted(index[r] for r in regions) for k, regions in covered.items()},
    "internal": sorted(internal),
}, open(os.path.join(OUT, "regions.json"), "w"))
json.dump({
    "tests": len(tests), "measured": len(covered), "failed": failed,
    "regions": len(universe), "cover_size": len(keep), "keep": keep,
    "rows": rows, "subsumed": subsumed,
}, open(os.path.join(OUT, "matrix.json"), "w"), indent=1)

print(f"tests {len(tests)}  measured {len(covered)}  failed/unmeasured {len(failed)}")
for key, code in failed:
    print(f"  FAILED (exit {code}): {key}")
print(f"regions covered {len(universe)}")
print(f"tests with no unique region: {len(redundant)}")
print(f"tests fully contained in one other test: {len(subsumed)}")
print(f"greedy minimal cover keeps {len(keep)} of {len(covered)} tests for the same coverage")
