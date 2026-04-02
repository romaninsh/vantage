#!/bin/bash
set -e

# Configuration
CONTAINER_NAME="surrealdb-local"
PORT="8000"
USER="root"
PASS="root"
NS="bakery"

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker and try again."
    exit 1
fi

# Stop existing container if running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping existing SurrealDB container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1 || true
fi

# Remove existing container
if docker ps -aq -f name=$CONTAINER_NAME | grep -q .; then
    echo "Removing existing SurrealDB container..."
    docker rm $CONTAINER_NAME > /dev/null 2>&1 || true
fi

echo "Starting SurrealDB local instance..."
echo "Container: $CONTAINER_NAME"
echo "Port: $PORT"
echo "Username: $USER"
echo "Password: $PASS"
echo "Namespace: $NS"

# Start SurrealDB container
docker run -d \
    --name $CONTAINER_NAME \
    -p $PORT:8000 \
    -e SURREAL_CAPS_ALLOW_EXPERIMENTAL=graphql \
    surrealdb/surrealdb:latest \
    start --log debug --user $USER --pass $PASS memory

# Wait for SurrealDB to be ready
echo "Waiting for SurrealDB to start..."
sleep 3

# Check if container is running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "✅ SurrealDB is running!"
    echo ""
    echo "Connection details:"
    echo "  Endpoint: ws://localhost:$PORT"
    echo "  Username: $USER"
    echo "  Password: $PASS"
    echo "  Namespace: $NS"
    echo ""
    echo "To connect with CLI:"
    echo "  surreal sql --endpoint ws://localhost:$PORT --username $USER --password $PASS --ns $NS"
    echo ""
    echo "To stop the instance, run: ./stop.sh"
else
    echo "❌ Failed to start SurrealDB container"
    docker logs $CONTAINER_NAME
    exit 1
fi
