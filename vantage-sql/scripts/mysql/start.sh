#!/bin/bash
set -e

# Configuration
CONTAINER_NAME="vantage-mysql"
PORT="3306"
USER="vantage"
PASS="vantage"
ROOT_PASS="vantage"
DB="vantage"

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker and try again."
    exit 1
fi

# Stop existing container if running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping existing MySQL container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1 || true
fi

# Remove existing container
if docker ps -aq -f name=$CONTAINER_NAME | grep -q .; then
    echo "Removing existing MySQL container..."
    docker rm $CONTAINER_NAME > /dev/null 2>&1 || true
fi

echo "Starting MySQL local instance..."
echo "Container: $CONTAINER_NAME"
echo "Port: $PORT"
echo "Username: $USER"
echo "Password: $PASS"
echo "Database: $DB"

# Start MySQL container
docker run -d \
    --name $CONTAINER_NAME \
    -p $PORT:3306 \
    -e MYSQL_ROOT_PASSWORD=$ROOT_PASS \
    -e MYSQL_USER=$USER \
    -e MYSQL_PASSWORD=$PASS \
    -e MYSQL_DATABASE=$DB \
    mysql:8

# Wait for MySQL to be ready
echo "Waiting for MySQL to start..."
for i in $(seq 1 60); do
    if docker exec $CONTAINER_NAME mysqladmin ping -h 127.0.0.1 -u$USER -p$PASS --silent > /dev/null 2>&1; then
        echo "MySQL is ready!"
        break
    fi
    sleep 1
done

# Check if container is running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo ""
    echo "Connection details:"
    echo "  URL: mysql://$USER:$PASS@localhost:$PORT/$DB"
    echo ""
    echo "To connect with mysql:"
    echo "  docker exec -it $CONTAINER_NAME mysql -u$USER -p$PASS $DB"
    echo ""
    echo "To stop the instance, run: ./stop.sh"
else
    echo "Failed to start MySQL container"
    docker logs $CONTAINER_NAME
    exit 1
fi
