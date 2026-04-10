#!/bin/bash
set -e

CONTAINER_NAME="mongo-vantage"

if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping MongoDB container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1
    echo "Stopped."
else
    echo "MongoDB container is not running."
fi
