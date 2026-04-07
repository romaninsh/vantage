#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="vantage-mysql"
USER="vantage"
PASS="vantage"
DB="vantage"

echo "Loading MySQL data..."

for db_file in "$SCRIPT_DIR"/db/*.sql; do
    db_name=$(basename "$db_file" .sql)
    echo "Loading $db_name..."
    docker exec -i $CONTAINER_NAME mysql -u$USER -p$PASS $DB < "$db_file"
    echo "  done."
done

echo "MySQL data loaded."
