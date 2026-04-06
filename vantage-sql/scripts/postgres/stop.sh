#!/bin/bash
set -e

CONTAINER_NAME="postgres-vantage"

if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping PostgreSQL container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1
    docker rm $CONTAINER_NAME > /dev/null 2>&1
    echo "PostgreSQL stopped."
else
    echo "PostgreSQL container is not running."
fi
