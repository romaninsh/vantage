#!/bin/bash
# Drop, recreate, and populate `vantage-demo-single-table` — one
# DynamoDB table holding 7 logical entities (product, version,
# deployment, environment, team, subscription, dataport) for the
# `dynamo-single-table` example.
set -e

ENDPOINT="${AWS_ENDPOINT_URL:-http://localhost:8000}"
TABLE="vantage-demo-single-table"

# AWS CLI requires *some* credentials even for local DynamoDB; fall back
# to harmless dummies so a fresh shell works without setup.
export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-local}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-local}"
export AWS_REGION="${AWS_REGION:-eu-west-2}"

aws_ddb() {
  aws dynamodb --endpoint-url "$ENDPOINT" "$@"
}

if aws_ddb describe-table --table-name "$TABLE" >/dev/null 2>&1; then
  echo "  removing existing $TABLE..."
  aws_ddb delete-table --table-name "$TABLE" >/dev/null
fi

echo "  creating $TABLE..."
aws_ddb create-table \
  --table-name "$TABLE" \
  --attribute-definitions \
      AttributeName=PK,AttributeType=S \
      AttributeName=SK,AttributeType=S \
  --key-schema \
      AttributeName=PK,KeyType=HASH \
      AttributeName=SK,KeyType=RANGE \
  --billing-mode PAY_PER_REQUEST \
  >/dev/null

echo "  populating $TABLE..."
aws_ddb batch-write-item \
  --request-items "file://$(dirname "$0")/single-table.items.json" \
  >/dev/null
