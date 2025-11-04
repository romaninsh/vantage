#!/bin/bash
set -e
source .env

# Clear previous test outputs
rm -rf test_outputs

failed=0
all_queries_have_data=true

for test_file in tests/*.surql; do
  db_name=$(basename "$test_file" .surql)
  echo "Testing $db_name..."

  # Create output directory for this test
  output_dir="test_outputs/$db_name"
  mkdir -p "$output_dir"

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

      # Write normalized result to individual query file, filtering out created_at lines
      echo "$result" | jq '.' | grep -v '"created_at"' | grep -v '"bakery": "bakery:hill_valley"' > "$output_dir/Query_$(printf "%02d" $query_num).json"

      if echo "$result" | grep -q 'Parse error'; then
        echo "FAIL: Query $query_num has parse errors"
        echo
        echo "$query"
        echo
        echo "Error: $result"
        failed=1
        all_queries_have_data=false
      elif echo "$result" | grep -q '\[\]'; then
        echo "EMPTY: Query $query_num returned no results"
        echo
        echo "$query"
        echo
        all_queries_have_data=false
      else
        echo "PASS: Query $query_num returned data"
      fi

      query=""
    fi
  done < "$test_file"

  echo ""
done

# Compare directories if we have both v1 and v2 and all queries returned data
if [[ -d "test_outputs/v1" && -d "test_outputs/v2" && "$all_queries_have_data" == "true" ]]; then
  echo "Comparing v1 and v2 outputs..."
  echo "=============================================="

  # First run diff -q to detect changes
  if diff -q test_outputs/v1 test_outputs/v2 >/dev/null ; then
      echo "No significant differences"
  else
    # Run difft to show colored output to console
    difft test_outputs/v1 test_outputs/v2 || true
  fi

elif [[ -d "test_outputs/v1" && -d "test_outputs/v2" ]]; then
  echo "Skipping comparison - some queries returned empty results or failed"
fi

exit $failed
