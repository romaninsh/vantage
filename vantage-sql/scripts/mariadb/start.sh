#!/bin/bash
# Start MariaDB 11 in the MySQL slot (same container name, port and
# credentials as ../mysql/start.sh), so the MySQL tests run against it.
# Load fixtures with ../mysql/ingress.sh; stop with ../mysql/stop.sh.
set -e

CONTAINER_NAME="vantage-mysql"
PORT="3306"
USER="vantage"
PASS="vantage"
ROOT_PASS="vantage"
DB="vantage"

if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker and try again."
    exit 1
fi

docker rm -f $CONTAINER_NAME > /dev/null 2>&1 || true

echo "Starting MariaDB 11 as $CONTAINER_NAME on port $PORT..."
docker run -d \
    --name $CONTAINER_NAME \
    -p $PORT:3306 \
    -e MARIADB_ROOT_PASSWORD=$ROOT_PASS \
    -e MARIADB_USER=$USER \
    -e MARIADB_PASSWORD=$PASS \
    -e MARIADB_DATABASE=$DB \
    mariadb:11 > /dev/null

for i in $(seq 1 60); do
    if docker exec $CONTAINER_NAME healthcheck.sh --connect --innodb_initialized > /dev/null 2>&1; then
        echo "MariaDB is ready: mysql://$USER:$PASS@localhost:$PORT/$DB"
        exit 0
    fi
    sleep 1
done

echo "MariaDB did not become ready"
docker logs $CONTAINER_NAME
exit 1
