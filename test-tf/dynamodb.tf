// DynamoDB tables for exercising vantage-aws/dynamodb against a real account.
//
// PAY_PER_REQUEST keeps cost ~zero when idle (no provisioned throughput).
// Both tables are tiny on purpose — they exist to be CRUD'd by integration
// tests, not to hold real workload data.

resource "aws_dynamodb_table" "products" {
  name         = "${var.name}-products"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "id"

  attribute {
    name = "id"
    type = "S"
  }

  tags = { Name = var.name }
}

resource "aws_dynamodb_table" "orders" {
  name         = "${var.name}-orders"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "customer_id"
  range_key    = "order_id"

  attribute {
    name = "customer_id"
    type = "S"
  }

  attribute {
    name = "order_id"
    type = "S"
  }

  tags = { Name = var.name }
}
