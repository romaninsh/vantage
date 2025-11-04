#!/bin/bash
set -e
source .env
for db_file in db/*.surql; do
  db_name=$(basename "$db_file" .surql)
  echo "Processing $db_name..."
  echo "REMOVE DATABASE $db_name;" | surreal sql --endpoint "$DB_ENDPOINT" --username "$DB_USER" --password "$DB_PASS" --auth-level "$DB_AUTH_LEVEL" --ns "$DB_NS" --hide-welcome --json 2>/dev/null || true
  echo "DEFINE DATABASE $db_name;" | surreal sql --endpoint "$DB_ENDPOINT" --username "$DB_USER" --password "$DB_PASS" --auth-level "$DB_AUTH_LEVEL" --ns "$DB_NS" --hide-welcome --json
  cat "$db_file" | surreal sql --endpoint "$DB_ENDPOINT" --username "$DB_USER" --password "$DB_PASS" --auth-level "$DB_AUTH_LEVEL" --ns "$DB_NS" --db "$db_name" --hide-welcome >/dev/null 2>&1
done
