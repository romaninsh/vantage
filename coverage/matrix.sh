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
# regular debug build. Needs cargo-llvm-cov and cargo-nextest. The per-test
# runs are coverage/matrix-rs, built here on first use.
#
#   MATRIX_TARGET_DIR=<dir>   use another instrumented target dir (one per lane)
#   MATRIX_ARCHIVE=<file>     run from an instrumented nextest archive (made by
#                             `cargo llvm-cov nextest-archive`) instead of building
#   MATRIX_RS=<file>          a prebuilt coverage/matrix-rs
#   JOBS=<n>                  tests run at once (default: every core)
set -euo pipefail

baseline=0
if [[ "${1:-}" = "--baseline" ]]; then baseline=1; shift; fi
pkg=$1
jaccard=${2:-0.9}
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/coverage/report/$pkg"
base="$root/coverage/report/$pkg.baseline"
rm -rf "$out"
mkdir -p "$out"
log="$out/run.log"

# Built before the coverage environment below, so the tool isn't instrumented.
# MATRIX_RS=<binary> uses a prebuilt one (CI builds it once per run).
matrix_rs=${MATRIX_RS:-$root/target/matrix-rs/release/matrix-rs}
if [[ -z "${MATRIX_RS:-}" ]]; then
  cargo build --release --quiet --manifest-path "$root/coverage/matrix-rs/Cargo.toml" \
    --target-dir "$root/target/matrix-rs"
fi

eval "$(cargo llvm-cov show-env --export-prefix)"
# MATRIX_TARGET_DIR gives a lane its own build, so a parallel lane rebuilding a
# shared dependency can't swap test binaries under a running matrix.
export CARGO_TARGET_DIR="${MATRIX_TARGET_DIR:-$root/target/llvm-cov-target}"

echo "building and listing $pkg tests (log: $log)"
# diorama's `bdd` is cucumber (harness = false) and can't be listed.
if [[ -n "${MATRIX_ARCHIVE:-}" ]]; then
  # The binaries must outlive the listing: matrix-rs runs them one test at a time.
  list=(cargo nextest list --archive-file "$MATRIX_ARCHIVE" --workspace-remap "$root"
    --extract-to "$CARGO_TARGET_DIR/archive-$pkg" --extract-overwrite
    -E "package($pkg) and not binary(bdd)")
else
  list=(cargo nextest list -p "$pkg" --all-features -E 'not binary(bdd)')
fi
if ! "${list[@]}" --message-format json > "$out/list.json" 2> "$log"; then
  echo "build failed, see $log" >&2
  exit 1
fi

dir=$(cargo metadata --format-version 1 --no-deps \
  | python3 -I -c 'import json, os, sys
m = json.load(sys.stdin)
print(next(os.path.dirname(p["manifest_path"]) for p in m["packages"] if p["name"] == sys.argv[1]))' "$pkg")

"$matrix_rs" "$out/list.json" "$dir" "$out" "${JOBS:-$(getconf _NPROCESSORS_ONLN)}"
python3 -I "$root/coverage/matrix.py" "$out" | tee -a "$log"
python3 -I "$root/coverage/report.py" "$out" "$dir" "$pkg" "$jaccard" | tee -a "$log"

if [[ "$baseline" = 1 ]]; then
  rm -rf "$base"
  cp -R "$out" "$base"
  # lost.py maps baseline regions onto the current sources through this copy.
  mkdir -p "$base/source"
  cp -R "$dir/src" "$base/source/src"
  echo "baseline saved: $base"
elif [[ -d "$base" ]]; then
  python3 -I "$root/coverage/lost.py" "$pkg" "$dir" "$base" "$out" | tee "$out/lost.txt"
fi
