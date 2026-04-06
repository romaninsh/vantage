#!/bin/bash
set -e

# Configuration
CONTAINER_NAME="postgres-vantage"
PORT="5433"
USER="vantage"
PASS="vantage"
DB="vantage"

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker and try again."
    exit 1
fi

# Stop existing container if running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping existing PostgreSQL container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1 || true
fi

# Remove existing container
if docker ps -aq -f name=$CONTAINER_NAME | grep -q .; then
    echo "Removing existing PostgreSQL container..."
    docker rm $CONTAINER_NAME > /dev/null 2>&1 || true
fi

echo "Starting PostgreSQL local instance..."
echo "Container: $CONTAINER_NAME"
echo "Port: $PORT"
echo "Username: $USER"
echo "Password: $PASS"
echo "Database: $DB"

# Start PostgreSQL container
docker run -d \
    --name $CONTAINER_NAME \
    -p $PORT:5432 \
    -e POSTGRES_USER=$USER \
    -e POSTGRES_PASSWORD=$PASS \
    -e POSTGRES_DB=$DB \
    postgres:17-alpine

# Wait for PostgreSQL to be ready
echo "Waiting for PostgreSQL to start..."
for i in $(seq 1 30); do
    if docker exec $CONTAINER_NAME pg_isready -U $USER -d $DB > /dev/null 2>&1; then
        echo "PostgreSQL is ready!"
        break
    fi
    sleep 1
done

# Check if container is running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo ""
    echo "Connection details:"
    echo "  URL: postgres://$USER:$PASS@localhost:$PORT/$DB"
    echo ""
    echo "To connect with psql:"
    echo "  docker exec -it $CONTAINER_NAME psql -U $USER -d $DB"
    echo ""
    echo "To stop the instance, run: ./stop.sh"
else
    echo "Failed to start PostgreSQL container"
    docker logs $CONTAINER_NAME
    exit 1
fi
