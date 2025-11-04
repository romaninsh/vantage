#!/bin/bash
set -e

# Configuration
CONTAINER_NAME="surrealdb-local"

echo "Stopping SurrealDB local instance..."

# Stop the container if it's running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping container $CONTAINER_NAME..."
    docker stop $CONTAINER_NAME
    echo "✅ Container stopped"
else
    echo "Container $CONTAINER_NAME is not running"
fi

# Remove the container
if docker ps -aq -f name=$CONTAINER_NAME | grep -q .; then
    echo "Removing container $CONTAINER_NAME..."
    docker rm $CONTAINER_NAME
    echo "✅ Container removed"
fi

echo "SurrealDB local instance stopped and cleaned up"
