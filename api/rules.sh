#!/usr/bin/env bash
# Rules the public-API snapshots in api/*.txt must keep. Run it after
# api/snapshot.sh; .github/workflows/public-api.yaml runs it after the diff.
#
# - A handle or view type has no public field. Its state is reached through
#   methods, so it can change without breaking a caller. Plain data (Tuning,
#   Fetched, FlashOutcome, WriteCapabilities, ViewStats, EnrichedRecord, ...)
#   may have public fields, and is left out of HANDLES.
# - No crate with a snapshot allows dead code in its src/lib.rs.
set -euo pipefail
cd "$(dirname "$0")/.."

HANDLES=(
  Lens LensBuilder Dio DioBuilder WeakDio LoaderCx ChunkSink Loader Refresher Cache Servo
  TableScenery RecordScenery ValueScenery CappedScenery TableSceneryBuilder ValueSceneryBuilder
  MemoryCache RedbCache MemoryCacheTable RedbCacheTable
)

types="$(IFS='|'; echo "${HANDLES[*]}")"
# A named field reads `pub <path>::<Type>::<field>: <type>`, with `<..>` after
# a generic type. A tuple field shows in the struct line:
# `pub struct <path>::<Type>(pub <type>)`.
field="^pub ([a-z0-9_]+::)*($types)(<[^>]*>)?::[a-z0-9_]+: |^pub struct ([a-z0-9_]+::)*($types)(<[^>]*>)?\(.*pub "

status=0
for snapshot in api/*.txt; do
  if grep -En "$field" "$snapshot"; then
    echo "$snapshot: a handle or view type has a public field (above)" >&2
    status=1
  fi
  lib="$(basename "$snapshot" .txt)/src/lib.rs"
  if grep -n 'allow(dead_code)' "$lib"; then
    echo "$lib: allows dead code (above)" >&2
    status=1
  fi
done
exit "$status"
