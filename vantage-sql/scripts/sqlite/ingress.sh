#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DB_FILE="${DB_FILE:-${SCRIPT_DIR}/../../../target/bakery.sqlite}"

echo "Setting up SQLite database at $DB_FILE..."

# Ensure parent directory exists
mkdir -p "$(dirname "$DB_FILE")"

# Remove existing database
rm -f "$DB_FILE"

for db_file in "$SCRIPT_DIR"/db/v[0-3]*.sql; do
  db_name=$(basename "$db_file" .sql)
  echo "Loading $db_name..."
  sqlite3 "$DB_FILE" < "$db_file"
  echo "  done."
done

echo "✅ SQLite database ready at $DB_FILE"
echo ""
echo "To connect:"
echo "  sqlite3 $DB_FILE"
