"""Regions one matrix run covered that a later run no longer does.

usage: python3 -I lost.py <package> <crate_dir> <baseline_dir> <run_dir> [--json]

Prints the lost regions grouped by file, as line ranges, so a test can be
written to cover them again. Paths coverage/config.yaml leaves out are
skipped, and a loss within its tolerance counts as run-to-run noise.

Baseline regions are mapped onto the current sources through the copy of
src/ the baseline keeps, so code that only moved isn't lost, and code that
was deleted (a test module, say) isn't lost either.
"""
import difflib
import json
import os
import sys
from collections import defaultdict

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import config  # noqa: E402

package, crate, base_dir, run_dir = sys.argv[1:5]
left_out = config.excluded(package)
tolerance = config.tolerance(package)
source = os.path.join(base_dir, "source")
if not os.path.isdir(source):
    sys.exit(f"{base_dir} has no source/ copy: re-run coverage/matrix.sh --baseline {package}")


def lines(path):
    return open(path, encoding="utf-8").read().splitlines() if os.path.exists(path) else []


def line_map(path):
    """Baseline line -> current line, for lines that survive unchanged."""
    old, new = lines(os.path.join(source, path)), lines(os.path.join(crate, path))
    mapping = {}
    matcher = difflib.SequenceMatcher(None, old, new, autojunk=False)
    for tag, i1, i2, j1, _ in matcher.get_opcodes():
        if tag == "equal":
            mapping.update({i + 1: j1 + i - i1 + 1 for i in range(i1, i2)})
    return mapping


maps, before, removed = {}, set(), 0
for path, ls, cs, le, ce in json.load(open(os.path.join(base_dir, "regions.json")))["regions"]:
    if left_out(path):
        continue
    if path not in maps:
        maps[path] = line_map(path)
    m = maps[path]
    if ls in m and le in m:
        before.add((path, m[ls], cs, m[le], ce))
    else:
        removed += 1
after = {tuple(r) for r in json.load(open(os.path.join(run_dir, "regions.json")))["regions"]
         if not left_out(r[0])}
lost = sorted(before - after)

by_file = defaultdict(list)
for path, ls, cs, le, ce in lost:
    by_file[path].append((ls, cs, le, ce))

if "--json" in sys.argv:
    json.dump({f: rs for f, rs in by_file.items()}, sys.stdout, indent=1)
    sys.exit(0)

print(f"{len(before)} regions before, {len(after)} after, {len(lost)} lost, {len(after - before)} gained"
      f" ({removed} baseline regions sat in code since deleted or changed, and don't count)")
if len(lost) <= tolerance:
    print(f"within the tolerance of {tolerance}: nothing to restore")
else:
    print(f"over the tolerance of {tolerance}: restore until no more than {tolerance} are lost")
for path in sorted(by_file, key=lambda p: -len(by_file[p])):
    ranges = by_file[path]
    lines = sorted({ls for ls, _, _, _ in ranges})
    spans, start, prev = [], lines[0], lines[0]
    for line in lines[1:]:
        if line != prev + 1:
            spans.append((start, prev))
            start = line
        prev = line
    spans.append((start, prev))
    text = ", ".join(f"{a}" if a == b else f"{a}-{b}" for a, b in spans)
    print(f"{len(ranges):5d}  {path}: {text}")
