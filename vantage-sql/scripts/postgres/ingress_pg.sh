#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="postgres-vantage"
USER="vantage"
DB="vantage_pg"

echo "Loading PostgreSQL-specific data into $DB..."

for db_file in "$SCRIPT_DIR"/db/v4*.sql; do
    db_name=$(basename "$db_file" .sql)
    echo "Loading $db_name..."
    docker exec -i $CONTAINER_NAME psql -U $USER -d $DB < "$db_file"
    echo "  done."
done

echo "PostgreSQL-specific data loaded into $DB."
