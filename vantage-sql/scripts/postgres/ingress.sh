#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="postgres-vantage"
USER="vantage"

echo "Loading PostgreSQL data..."

for db_file in "$SCRIPT_DIR"/db/v*.sql; do
    db_name=$(basename "$db_file" .sql)
    db="vantage_${db_name}"
    echo "Resetting database $db and loading $db_name..."
    docker exec -i $CONTAINER_NAME psql -U $USER -d postgres \
        -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '$db' AND pid <> pg_backend_pid();" \
        -c "DROP DATABASE IF EXISTS \"$db\";" \
        -c "CREATE DATABASE \"$db\" OWNER $USER;"
    docker exec -i $CONTAINER_NAME psql -U $USER -d $db < "$db_file"
    echo "  done."
done

echo "PostgreSQL data loaded."
