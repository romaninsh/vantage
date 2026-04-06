#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="postgres-vantage"
USER="vantage"
DB="vantage"

echo "Loading PostgreSQL data..."

for db_file in "$SCRIPT_DIR"/db/*.sql; do
    db_name=$(basename "$db_file" .sql)
    echo "Loading $db_name..."
    docker exec -i $CONTAINER_NAME psql -U $USER -d $DB < "$db_file"
    echo "  done."
done

echo "PostgreSQL data loaded."
