output "cluster_name" {
  value = aws_ecs_cluster.main.name
}

output "cluster_arn" {
  value = aws_ecs_cluster.main.arn
}

output "service_name" {
  value = aws_ecs_service.main.name
}

output "task_definition_arn" {
  value = aws_ecs_task_definition.nginx.arn
}

output "log_group_name" {
  value = aws_cloudwatch_log_group.main.name
}

output "vpc_id" {
  value = aws_vpc.main.id
}

output "subnet_ids" {
  value = aws_subnet.public[*].id
}

output "dynamodb_products_table" {
  value = aws_dynamodb_table.products.name
}

output "dynamodb_orders_table" {
  value = aws_dynamodb_table.orders.name
}
