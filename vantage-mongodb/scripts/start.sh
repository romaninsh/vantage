#!/bin/bash
set -e

# Configuration
CONTAINER_NAME="mongo-vantage"
PORT="27017"
DB="vantage"

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker and try again."
    exit 1
fi

# Stop existing container if running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping existing MongoDB container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1 || true
fi

# Remove existing container
if docker ps -aq -f name=$CONTAINER_NAME | grep -q .; then
    echo "Removing existing MongoDB container..."
    docker rm $CONTAINER_NAME > /dev/null 2>&1 || true
fi

echo "Starting MongoDB local instance..."
echo "Container: $CONTAINER_NAME"
echo "Port: $PORT"
echo "Database: $DB"

# Start MongoDB container as a single-node replica set (no auth for local dev).
# Change streams and transactions need a replica set.
docker run -d \
    --name $CONTAINER_NAME \
    -p $PORT:27017 \
    mongo:7 --replSet rs0 --bind_ip_all

# Wait for MongoDB to accept commands
echo "Waiting for MongoDB to start..."
for i in $(seq 1 30); do
    if docker exec $CONTAINER_NAME mongosh --quiet --eval "db.runCommand({ping:1})" > /dev/null 2>&1; then
        break
    fi
    sleep 1
done

echo "Initiating replica set rs0..."
docker exec $CONTAINER_NAME mongosh --quiet --eval \
    'try { rs.status() } catch (e) { rs.initiate({_id:"rs0",members:[{_id:0,host:"localhost:27017"}]}) }' \
    > /dev/null

# Wait for the node to become primary
PRIMARY=false
for i in $(seq 1 30); do
    if docker exec $CONTAINER_NAME mongosh --quiet --eval "db.hello().isWritablePrimary" 2>/dev/null | grep -q true; then
        echo "MongoDB is ready!"
        PRIMARY=true
        break
    fi
    sleep 1
done

if [ "$PRIMARY" != true ]; then
    echo "Error: MongoDB node never became primary"
    docker logs $CONTAINER_NAME
    exit 1
fi

if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo ""
    echo "Connection details:"
    echo "  URL: mongodb://localhost:$PORT/?directConnection=true"
    echo "  Database: $DB"
    echo ""
    echo "To connect with mongosh:"
    echo "  docker exec -it $CONTAINER_NAME mongosh $DB"
    echo ""
    echo "To stop: ./stop.sh"
else
    echo "Failed to start MongoDB container"
    docker logs $CONTAINER_NAME
    exit 1
fi
