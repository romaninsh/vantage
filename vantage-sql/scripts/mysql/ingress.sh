#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONTAINER_NAME="vantage-mysql"
USER="vantage"
PASS="vantage"
ROOT_PASS="vantage"

# MariaDB 11 images ship only the `mariadb` client; MySQL images only `mysql`.
if docker exec $CONTAINER_NAME sh -c 'command -v mariadb' > /dev/null 2>&1; then
    CLIENT="mariadb"
else
    CLIENT="mysql"
fi

echo "Loading MySQL data (client: $CLIENT)..."

# The shared `vantage` db carries the v2/v3 fixtures, as in CI.
docker exec -i $CONTAINER_NAME $CLIENT -uroot -p$ROOT_PASS -e \
    "DROP DATABASE IF EXISTS vantage; CREATE DATABASE vantage; GRANT ALL PRIVILEGES ON vantage.* TO '$USER'@'%';"
for db_file in "$SCRIPT_DIR"/db/v[0-3]*.sql; do
    echo "Loading $(basename "$db_file") into vantage..."
    docker exec -i $CONTAINER_NAME $CLIENT -u$USER -p$PASS vantage < "$db_file"
done

for db_file in "$SCRIPT_DIR"/db/v*.sql; do
    db_name=$(basename "$db_file" .sql)
    db="vantage_${db_name}"
    echo "Resetting database $db and loading $db_name..."
    docker exec -i $CONTAINER_NAME $CLIENT -uroot -p$ROOT_PASS -e \
        "DROP DATABASE IF EXISTS \`$db\`; CREATE DATABASE \`$db\`; GRANT ALL PRIVILEGES ON \`$db\`.* TO '$USER'@'%';"
    docker exec -i $CONTAINER_NAME $CLIENT -u$USER -p$PASS $db < "$db_file"
    echo "  done."
done

echo "MySQL data loaded."
