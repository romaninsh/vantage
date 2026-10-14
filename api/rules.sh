#!/usr/bin/env bash
# Rules the public-API snapshots in api/*.txt must keep. Run it after
# api/snapshot.sh; .github/workflows/public-api.yaml runs it after the diff.
# SNAPSHOTS=/tmp/x/vantage-vista.txt points it at other files.
#
# - A handle or view type has no public field. Its state is reached through
#   methods, so it can change without breaking a caller.
# - No crate with a snapshot allows dead code in its src/lib.rs.
# - Rule 7: a public enum, or a struct with a public field, is
#   #[non_exhaustive] unless it is listed in CLOSED.
# - Rule 11: an item has one path outside the prelude (a name with two paths
#   is listed in SAME_NAME), every path starts with the snapshot's crate, and
#   only the prelude re-exports (`pub use`).
set -euo pipefail
cd "$(dirname "$0")/.."

HANDLES=(
  Lens LensBuilder Dio DioBuilder WeakDio LoaderCx ChunkSink Loader Refresher Cache Servo
  TableScenery RecordScenery ValueScenery TableSceneryBuilder ValueSceneryBuilder
  MemoryCache RedbCache MemoryCacheTable RedbCacheTable
)
CLOSED=(
  vantage_vista::SortDirection # two directions
  vantage_diorama::Generation  # a counter newtype
)
SAME_NAME=()

types="$(IFS='|'; echo "${HANDLES[*]}")"
# A named field reads `pub <path>::<Type>::<field>: <type>`, with `<..>` after
# a generic type. A tuple field shows in the struct line:
# `pub struct <path>::<Type>(pub <type>)`. Attributes (`#[repr(u8)] `) may
# lead any line.
attrs='^(#\[[^]]*\] )*'
field="${attrs}pub ([a-z0-9_]+::)*($types)(<[^>]*>)?::[a-z0-9_]+: |${attrs}pub struct ([a-z0-9_]+::)*($types)(<[^>]*>)?\(.*pub "

# Item lines name a path; a field line `pub <struct path>::<field>: ` marks its
# struct. Only paths whose parents are all modules are items (not methods); a
# method of an impl on a primitive (`pub fn u64::from`) reads like one. CLOSED
# names the item's own path, so its prelude path is read without `prelude::`.
check='
BEGIN { split(closed, a, " "); for (i in a) isclosed[a[i]] = 1
        split(same, a, " "); for (i in a) issame[a[i]] = 1 }
{ line = $0; ne = 0
  while (match(line, /^#\[[^]]*\] /)) {
    if (substr(line, 1, RLENGTH) == "#[non_exhaustive] ") ne = 1
    line = substr(line, RLENGTH + 1)
  } }
match(line, /^pub ((async|const|unsafe) )*(struct|enum|trait|type|fn|const|static|macro|use) /) {
  n = split(substr(line, 1, RLENGTH - 1), w, " "); kind = w[n]
  rest = substr(line, RLENGTH + 1); match(rest, /^[A-Za-z0-9_:]+/)
  path = substr(rest, 1, RLENGTH); sub(/:+$/, "", path)
  k = split(path, s, "::"); name = s[k]
  own = path; sub(/::prelude::/, "::", own)
  if (kind == "use" && path !~ /::prelude::/) bad(NR, "rule 11: " path " is a re-export outside the prelude")
  if (kind == "enum" && !ne && !(own in isclosed)) bad(NR, "rule 7: enum " path " is not #[non_exhaustive]")
  if (kind == "struct" && !ne && !(own in isclosed)) {
    if (rest ~ /\(.*pub /) bad(NR, "rule 7: struct " path " has a public field and is not #[non_exhaustive]")
    else open[path] = NR
  }
  for (i = 1; i < k; i++) if (s[i] !~ /^[a-z_][a-z0-9_]*$/) next
  if (k < 2 || s[1] ~ /^([iu](8|16|32|64|128|size)|f32|f64|bool|char|str)$/) next
  if (s[1] != crate) bad(NR, "rule 11: " path " is outside " crate)
  if (path ~ /::prelude::/ || ((name, path) in seen)) next
  seen[name, path] = 1; paths[name] = paths[name] " " path; count[name]++
  next
}
match(line, /^pub [A-Za-z0-9_:]+: /) { p = substr(line, 5, RLENGTH - 6); sub(/::[a-z0-9_]+$/, "", p); pubfield[p] = 1 }
END {
  for (p in open) if (p in pubfield) bad(open[p], "rule 7: struct " p " has a public field and is not #[non_exhaustive]")
  for (n in count) if (count[n] > 1 && !(n in issame)) bad(0, "rule 11: " n " has " count[n] " paths:" paths[n])
  exit failed
}
function bad(nr, msg) { printf "%s:%d: %s\n", FILENAME, nr, msg > "/dev/stderr"; failed = 1 }
'

status=0
for snapshot in ${SNAPSHOTS:-api/*.txt}; do
  if grep -En "$field" "$snapshot"; then
    echo "$snapshot: a handle or view type has a public field (above)" >&2
    status=1
  fi
  crate="$(basename "$snapshot" .txt)"
  if grep -n 'allow(dead_code)' "$crate/src/lib.rs"; then
    echo "$crate/src/lib.rs: allows dead code (above)" >&2
    status=1
  fi
  awk -v crate="${crate//-/_}" -v closed="${CLOSED[*]}" -v same="${SAME_NAME[*]-}" "$check" "$snapshot" || status=1
done
exit "$status"
