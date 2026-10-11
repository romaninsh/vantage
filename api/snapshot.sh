#!/usr/bin/env bash
# Regenerates the public-API snapshots in api/<crate>.txt.
#
# This header is the one place that pins the toolchain. cargo-public-api reads
# rustdoc JSON, which only nightly emits, and the JSON format changes between
# nightlies, so both versions are fixed to make local runs and CI agree.
# .github/workflows/public-api.yaml reads both lines below. Bump them together,
# then regenerate.
set -euo pipefail

NIGHTLY="nightly-2026-10-09"
PUBLIC_API_VERSION="0.52.0"

CRATES=(
  vantage-vista
  vantage-vista-factory
  vantage-diorama
  vantage-diorama-aggregate
)

cd "$(dirname "$0")/.."

installed="$(cargo public-api --version 2>/dev/null || true)"
if [[ "$installed" != "cargo-public-api $PUBLIC_API_VERSION" ]]; then
  echo "need cargo-public-api $PUBLIC_API_VERSION (cargo install cargo-public-api --locked --version $PUBLIC_API_VERSION), found: ${installed:-none}" >&2
  exit 1
fi

# A snapshot for a crate missing from CRATES would never be regenerated.
for file in api/*.txt; do
  name="$(basename "$file" .txt)"
  if [[ " ${CRATES[*]} " != *" $name "* ]]; then
    echo "$file has no entry in CRATES in api/snapshot.sh; add the crate or delete the file" >&2
    exit 1
  fi
done

rustup toolchain install "$NIGHTLY" --profile minimal --no-self-update >/dev/null

# `-s` omits blanket impls (`impl<T> Any for T` and the like), which only
# restate std. Auto-trait and derived impls stay: Send, Sync and Clone are API.
for crate in "${CRATES[@]}"; do
  cargo "+$NIGHTLY" public-api -p "$crate" --all-features -s --color never >"api/$crate.txt"
done
