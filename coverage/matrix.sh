#!/usr/bin/env bash
# Per-test coverage matrix for one package: every test runs alone with its own
# LLVM profile, and the overlap between tests becomes reports.
#
#   coverage/matrix.sh [--baseline] <package> [jaccard]   # default jaccard 0.9
#
# Writes coverage/report/<package>/:
#   index.md         totals, chunk list
#   clusters-NN.md   overlapping blackbox tests, 8 clusters per file, with a
#                    decision column to fill in
#   internal.md      every test under src/, by file, with line ranges
#   run.log          build and test output
# For an HTML view (people, not agents): coverage/report_html.py on the run dir.
# --baseline also copies the run, and the crate's src/, to
# coverage/report/<package>.baseline/; every later run prints what it lost
# against that baseline.
#
# The instrumented build has its own target dir, so it never replaces the
# regular debug build. Needs cargo-llvm-cov and cargo-nextest.
set -euo pipefail

baseline=0
if [ "${1:-}" = "--baseline" ]; then baseline=1; shift; fi
pkg=$1
jaccard=${2:-0.9}
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/coverage/report/$pkg"
base="$root/coverage/report/$pkg.baseline"
rm -rf "$out"
mkdir -p "$out"
log="$out/run.log"

eval "$(cargo llvm-cov show-env --export-prefix)"
export CARGO_TARGET_DIR="$root/target/llvm-cov-target"

echo "building and listing $pkg tests (log: $log)"
# diorama's `bdd` is cucumber (harness = false) and can't be listed.
if ! cargo nextest list -p "$pkg" --all-features -E 'not binary(bdd)' \
  --message-format json > "$out/list.json" 2> "$log"; then
  echo "build failed, see $log" >&2
  exit 1
fi

dir=$(cargo metadata --format-version 1 --no-deps \
  | python3 -I -c 'import json, os, sys
m = json.load(sys.stdin)
print(next(os.path.dirname(p["manifest_path"]) for p in m["packages"] if p["name"] == sys.argv[1]))' "$pkg")

python3 -I "$root/coverage/matrix.py" "$out/list.json" "$dir" "$out" "${JOBS:-10}" | tee -a "$log"
python3 -I "$root/coverage/report.py" "$out" "$dir" "$jaccard" | tee -a "$log"

if [ "$baseline" = 1 ]; then
  rm -rf "$base"
  cp -R "$out" "$base"
  # lost.py maps baseline regions onto the current sources through this copy.
  mkdir -p "$base/source"
  cp -R "$dir/src" "$base/source/src"
  echo "baseline saved: $base"
elif [ -d "$base" ]; then
  python3 -I "$root/coverage/lost.py" "$pkg" "$dir" "$base" "$out" | tee "$out/lost.txt"
fi
