#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="mongo-vantage"
DB="vantage"

echo "Loading MongoDB data..."

for db_file in "$SCRIPT_DIR"/db/v[0-3]*.js; do
    db_name=$(basename "$db_file" .js)
    echo "Loading $db_name..."
    docker exec -i $CONTAINER_NAME mongosh $DB < "$db_file"
    echo "  done."
done

echo "MongoDB data loaded."
