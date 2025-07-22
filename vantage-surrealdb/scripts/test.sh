#!/bin/bash
set -e
source .env
failed=0
for test_file in tests/*.surql; do
  db_name=$(basename "$test_file" .surql)
  echo "Testing $db_name..."

  # Execute each query separately to identify which ones return empty results
  query_num=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    # Skip comments and empty lines
    if [[ "$line" =~ ^[[:space:]]*-- ]] || [[ -z "${line// }" ]]; then
      continue
    fi

    # Accumulate multi-line queries
    query="$query $line"

    # If line ends with semicolon, execute the query
    if [[ "$line" =~ \;[[:space:]]*$ ]]; then
      query_num=$((query_num + 1))

      result=$(echo "$query" | surreal sql --endpoint "$DB_ENDPOINT" --username "$DB_USER" --password "$DB_PASS" --auth-level "$DB_AUTH_LEVEL" --ns "$DB_NS" --db "$db_name" --hide-welcome --json)

      if echo "$result" | grep -q 'Parse error'; then
        echo "FAIL: Query $query_num has parse errors"
        echo
        echo "$query"
        echo
        echo "Error: $result"
        failed=1
      elif echo "$result" | grep -q '\[\]'; then
        echo "EMPTY: Query $query_num returned no results"
        echo
        echo "$query"
        echo
      else
        echo "PASS: Query $query_num returned data"
      fi

      query=""
    fi
  done < "$test_file"

  echo ""
done
exit $failed
