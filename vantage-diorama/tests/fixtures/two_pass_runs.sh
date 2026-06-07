#!/usr/bin/env bash
# Dummy two-pass data source for the diorama BDD/integration suite.
#
#   list  <offset> <limit> [branch]  -> JSON array of {id, branch} for the
#                                        (optionally branch-filtered) window
#   detail <id>                      -> [{id, detail:"full-<id>"}]
#
# Every invocation appends a line to $RUNS_LOG so the test can assert the exact
# sequence and count of list/detail calls the two-pass machinery issued.
set -euo pipefail

mode="${1:-}"
log="${RUNS_LOG:-}"

# Fixed dataset: five runs across two branches.
ids=(r0 r1 r2 r3 r4)
branches=(main dev main dev main)

case "$mode" in
  list)
    offset="${2:-0}"
    limit="${3:-50}"
    branch="${4:-}"
    [ -n "$log" ] && echo "list offset=$offset limit=$limit branch=${branch}" >> "$log"

    # Apply the branch filter first, then the offset/limit window.
    fids=()
    fbr=()
    for i in "${!ids[@]}"; do
      if [ -z "$branch" ] || [ "${branches[$i]}" = "$branch" ]; then
        fids+=("${ids[$i]}")
        fbr+=("${branches[$i]}")
      fi
    done

    n=${#fids[@]}
    end=$((offset + limit))
    out="["
    first=1
    j=$offset
    while [ "$j" -lt "$end" ] && [ "$j" -lt "$n" ]; do
      [ "$first" -eq 0 ] && out+=","
      out+="{\"id\":\"${fids[$j]}\",\"branch\":\"${fbr[$j]}\"}"
      first=0
      j=$((j + 1))
    done
    out+="]"
    printf '%s' "$out"
    ;;
  detail)
    id="${2:-}"
    [ -n "$log" ] && echo "detail id=$id" >> "$log"
    # A configured FAIL_ID emits invalid JSON so the detail pass sees an error
    # for exactly that id.
    if [ -n "${FAIL_ID:-}" ] && [ "$id" = "$FAIL_ID" ]; then
      printf 'BOOM-not-json'
      exit 0
    fi
    printf '[{"id":"%s","detail":"full-%s"}]' "$id" "$id"
    ;;
  *)
    printf '[]'
    ;;
esac
