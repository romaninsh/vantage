variable "region" {
  type        = string
  default     = "eu-west-2"
  description = "AWS region. Default matches the user's [default] profile."
}

variable "name" {
  type        = string
  default     = "vantage-demo"
  description = "Prefix used for every resource name + tag in this stack."
}

variable "desired_count" {
  type        = number
  default     = 1
  description = <<-EOT
    ECS service desired task count. 1 = ~$0.01/hr while running.
    Set to 0 if you want the cluster + service registered without
    incurring Fargate cost — list-clusters / list-services /
    list-task-defs will still have data, list-tasks will be empty.
  EOT
}

variable "cpu" {
  type    = string
  default = "256"
}

variable "memory" {
  type    = string
  default = "512"
}
