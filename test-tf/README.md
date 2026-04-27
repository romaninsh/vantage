# test-tf

Throwaway Terraform stack to give `vantage-aws-cli` something to explore.

## What it builds

In `eu-west-2` by default:

- VPC + 2 public subnets + IGW + security group (port 80 open).
- ECS cluster `vantage-demo` (Fargate + Fargate Spot capacity providers).
- Task definition `vantage-demo` — `nginx:stable-alpine`, 256 CPU / 512 MB.
- ECS service `vantage-demo-svc`, `desired_count = 1`, runs on **Fargate Spot** by default.
- CloudWatch log group `/ecs/vantage-demo` (7-day retention).
- IAM task-execution role.

## Cost

`desired_count = 1` runs one Fargate **Spot** task continuously: roughly **$0.003/hr**, **$0.07/day**, **$2/month**. Spot tasks can be reclaimed by AWS with a 2-minute warning — fine for a throwaway demo, not for production. ECS clusters themselves cost nothing; only the running task is billable.

Set `desired_count = 0` if you want everything provisioned without any running task at all.

```sh
terraform apply -var desired_count=0    # registered but quiet, cheap
terraform apply -var desired_count=1    # one running task, billable
```

## Usage

You'll need [`terraform`](https://developer.hashicorp.com/terraform/downloads) (or `tofu`):

```sh
brew install terraform     # or: brew install opentofu
```

Then:

```sh
cd test-tf
terraform init
terraform plan             # see what will be created
terraform apply            # create everything
# … explore with vantage-aws-cli, see outputs …
terraform destroy          # tear it all down
```

## Explore with vantage-aws-cli

`terraform apply` prints an `explore_with_vantage_aws_cli` output with copy-pasteable commands. The short version:

```sh
cd ../vantage-aws
cargo run --example aws-cli -- --region eu-west-2 list-clusters
cargo run --example aws-cli -- --region eu-west-2 list-services vantage-demo
cargo run --example aws-cli -- --region eu-west-2 list-tasks vantage-demo
cargo run --example aws-cli -- --region eu-west-2 list-task-defs --family-prefix vantage-demo
cargo run --example aws-cli -- --region eu-west-2 list-streams /ecs/vantage-demo
```

Once a Fargate task has started and pulled an image, `list-streams` will show the per-task log streams; `list-events /ecs/vantage-demo` will show nginx startup output.

## Customising

```sh
terraform apply -var region=us-east-1 -var name=my-demo -var desired_count=0
```

Variables (see `variables.tf`): `region`, `name`, `desired_count`, `cpu`, `memory`.
