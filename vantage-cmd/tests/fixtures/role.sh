#!/bin/sh
# Test stub for two-role (list + detail) scripts. Same locked command,
# different argv built by each role's script:
#   $1 = "list"                    -> id-only stub rows
#   $1 = "detail", $2 = id, $3 = x -> the full record for that id, echoing x
case "$1" in
  list)   printf '[{"id":"a"},{"id":"b"}]' ;;
  detail) printf '[{"id":"%s","detail":"full-%s","echoed":"%s"}]' "$2" "$2" "$3" ;;
  *)      printf '[]' ;;
esac
