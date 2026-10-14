"""Per-test coverage matrix for one crate. Run through coverage/matrix.sh.

usage: python3 -I matrix.py <list.json> <crate_dir> <out_dir> [jobs]

Runs every non-ignored test from `cargo nextest list --message-format json`
alone, with its own LLVM profile, exports the code regions it covers inside
<crate_dir>/src, and reports which tests cover nothing no other test covers.
"""
import json
import os
import subprocess
import sys
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor

LIST, CRATE, OUT = sys.argv[1], os.path.abspath(sys.argv[2]), os.path.abspath(sys.argv[3])
JOBS = int(sys.argv[4]) if len(sys.argv) > 4 else 8
SYSROOT = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
HOST = [l.split(": ")[1] for l in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if l.startswith("host")][0]
BIN = os.path.join(SYSROOT, "lib", "rustlib", HOST, "bin")
PROFDATA, LLVMCOV = os.path.join(BIN, "llvm-profdata"), os.path.join(BIN, "llvm-cov")
SRC = os.path.join(CRATE, "src")
os.makedirs(os.path.join(OUT, "prof"), exist_ok=True)

suites = json.load(open(LIST))["rust-suites"]
tests = []
for suite in suites.values():
    for name, case in suite["testcases"].items():
        if case.get("ignored") or not case.get("filter-match", {}).get("status") == "matches":
            continue
        tests.append((suite["binary-id"], suite["binary-path"], suite["cwd"], name))

sources = []
for root, _, files in os.walk(SRC):
    sources += [os.path.join(root, f) for f in files if f.endswith(".rs")]


def run(idx_test):
    idx, (bid, binary, cwd, name) = idx_test
    raw = os.path.join(OUT, "prof", f"{idx}.profraw")
    data = os.path.join(OUT, "prof", f"{idx}.profdata")
    env = dict(os.environ, LLVM_PROFILE_FILE=raw, RUST_TEST_THREADS="1")
    res = subprocess.run([binary, "--exact", name], cwd=cwd, env=env,
                         stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    if not os.path.exists(raw):
        return (bid, name, res.returncode, None)
    subprocess.run([PROFDATA, "merge", "-sparse", raw, "-o", data], check=True)
    exp = subprocess.run([LLVMCOV, "export", "-format=text", f"-instr-profile={data}",
                          binary, *sources], capture_output=True, text=True)
    os.remove(raw)
    os.remove(data)
    regions = set()
    for fn in json.loads(exp.stdout)["data"][0]["functions"]:
        files = fn["filenames"]
        for r in fn["regions"]:
            ls, cs, le, ce, count, file_id, _exp, kind = r[:8]
            path = files[file_id]
            if count > 0 and kind == 0 and path.startswith(SRC):
                regions.add((os.path.relpath(path, CRATE), ls, cs, le, ce))
    return (bid, name, res.returncode, regions)


with ThreadPoolExecutor(JOBS) as pool:
    results = list(pool.map(run, enumerate(tests)))

covered = {}
failed = []
for bid, name, code, regions in results:
    key = f"{bid}::{name}"
    if code != 0 or regions is None:
        failed.append((key, code))
    if regions is not None:
        covered[key] = regions

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
    best = max(pool_left, key=lambda k: len(pool_left[k] & remaining))
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
