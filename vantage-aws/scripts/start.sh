#!/bin/bash
set -e

# Configuration
CONTAINER_NAME="dynamodb-local"
PORT="8000"
IMAGE="amazon/dynamodb-local:latest"

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker and try again."
    exit 1
fi

# Stop existing container if running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "Stopping existing DynamoDB container..."
    docker stop $CONTAINER_NAME > /dev/null 2>&1 || true
fi

# Remove existing container
if docker ps -aq -f name=$CONTAINER_NAME | grep -q .; then
    echo "Removing existing DynamoDB container..."
    docker rm $CONTAINER_NAME > /dev/null 2>&1 || true
fi

echo "Starting DynamoDB local instance..."
echo "Container: $CONTAINER_NAME"
echo "Port: $PORT"
echo "Image: $IMAGE"

# Start DynamoDB Local container — `-inMemory` keeps everything in RAM
# so a fresh container starts empty every time.
docker run -d \
    --name $CONTAINER_NAME \
    -p $PORT:8000 \
    $IMAGE \
    -jar DynamoDBLocal.jar -inMemory -sharedDb

# Wait for DynamoDB to be ready
echo "Waiting for DynamoDB to start..."
sleep 2

# Check if container is running
if docker ps -q -f name=$CONTAINER_NAME | grep -q .; then
    echo "✅ DynamoDB Local is running!"
    echo ""
    echo "Connection details:"
    echo "  Endpoint:   http://localhost:$PORT"
    echo "  Region:     any (defaults to eu-west-2 in scripts/ingress.sh)"
    echo "  Creds:      any access/secret values are accepted"
    echo ""
    echo "To populate fixtures:"
    echo "  ./ingress.sh"
    echo ""
    echo "To run the example against this container:"
    echo "  export AWS_ENDPOINT_URL=http://localhost:$PORT"
    echo "  export AWS_ACCESS_KEY_ID=local AWS_SECRET_ACCESS_KEY=local AWS_REGION=eu-west-2"
    echo "  cargo run --example dynamo-single-table -p vantage-aws -- products"
    echo ""
    echo "To stop the instance, run: ./stop.sh"
else
    echo "❌ Failed to start DynamoDB container"
    docker logs $CONTAINER_NAME
    exit 1
fi
