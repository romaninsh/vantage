#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="postgres-vantage"
USER="vantage"

echo "Loading PostgreSQL data..."

reset_db() {
    docker exec -i $CONTAINER_NAME psql -U $USER -d postgres \
        -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '$1' AND pid <> pg_backend_pid();" \
        -c "DROP DATABASE IF EXISTS \"$1\";" \
        -c "CREATE DATABASE \"$1\" OWNER $USER;"
}

# The plain `vantage` database holds v0..v3 together, as CI loads it.
echo "Resetting database vantage and loading v0..v3..."
reset_db vantage
for db_file in "$SCRIPT_DIR"/db/v[0-3]*.sql; do
    docker exec -i $CONTAINER_NAME psql -U $USER -d vantage < "$db_file"
done
echo "  done."

for db_file in "$SCRIPT_DIR"/db/v*.sql; do
    db_name=$(basename "$db_file" .sql)
    db="vantage_${db_name}"
    echo "Resetting database $db and loading $db_name..."
    reset_db "$db"
    docker exec -i $CONTAINER_NAME psql -U $USER -d $db < "$db_file"
    echo "  done."
done

echo "PostgreSQL data loaded."
