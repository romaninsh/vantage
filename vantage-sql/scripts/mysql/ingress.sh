#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="vantage-mysql"
USER="vantage"
PASS="vantage"
ROOT_PASS="vantage"

echo "Loading MySQL data..."

for db_file in "$SCRIPT_DIR"/db/v*.sql; do
    db_name=$(basename "$db_file" .sql)
    db="vantage_${db_name}"
    echo "Resetting database $db and loading $db_name..."
    docker exec -i $CONTAINER_NAME mysql -uroot -p$ROOT_PASS -e \
        "DROP DATABASE IF EXISTS \`$db\`; CREATE DATABASE \`$db\`; GRANT ALL PRIVILEGES ON \`$db\`.* TO '$USER'@'%';"
    docker exec -i $CONTAINER_NAME mysql -u$USER -p$PASS $db < "$db_file"
    echo "  done."
done

echo "MySQL data loaded."
