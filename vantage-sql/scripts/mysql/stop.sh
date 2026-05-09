#!/bin/bash
set -e

CONTAINER_NAME="vantage-mysql"

if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping MySQL container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1
    docker rm $CONTAINER_NAME > /dev/null 2>&1
    echo "MySQL stopped."
else
    echo "MySQL container is not running."
fi
