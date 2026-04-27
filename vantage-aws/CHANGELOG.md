# Changelog

## 0.4.1 — 2026-04-28

More built-in models — CloudWatch `LogStream`, plus an ECS submodule covering clusters, services, tasks, and task definitions. The `parse_records` path now wraps scalar array elements (which is what AWS's ECS `List*` APIs return) so they look like ordinary single-field rows.

- New `vantage_aws::models::log_stream` — `LogStream` + `log_streams_table` (`DescribeLogStreams`, requires `logGroupName`). `LogStream::ref_events` derives the log group from the stream's ARN; `ref_events_in` lets the caller pass it. Both narrow `FilterLogEvents` via `logStreamNamePrefix`.
- `LogGroup` gains a `streams` `with_many` relation alongside the existing `events` one, plus a typed `LogGroup::ref_streams()`.
- New `vantage_aws::models::ecs` submodule:
  - `Cluster` + `clusters_table` (`ListClusters`). Methods: `name()` (parsed from ARN), `ref_services()`, `ref_tasks()`.
  - `Service` + `services_table` (`ListServices`, requires `cluster` condition). `name()` helper.
  - `Task` + `tasks_table` (`ListTasks`, requires `cluster` condition; supports `serviceName` / `family` / `desiredStatus` filters). `task_id()` helper. `ref_log_events(aws, log_group_name, prefix)` returns a `FilterLogEvents` table narrowed by `logStreamNamePrefix` — caller supplies the prefix because ECS streams put `taskId` at the *end* of the name (`<streamPrefix>/<container>/<taskId>`) and AWS only supports prefix matching.
  - `TaskDefinition` + `task_definitions_table` (`ListTaskDefinitions`).
- `parse_records` wraps scalar (string / number) array entries as `{<id_field>: value}` records — the ECS `List*` APIs return arrays of ARN strings rather than objects, so without this wrap the response would error.
- `examples/aws-cli.rs` gains `list-streams`, `traverse-streams`, `list-clusters`, `list-services`, `list-tasks` (with `--service` / `--family` / `--status`), `list-task-defs` (with `--family-prefix`).

## 0.4.0 — 2026-04-27

First release — incubating. Read-only AWS JSON-1.1 RPC backend that exposes AWS APIs (CloudWatch Logs, ECS, DynamoDB control plane, KMS, …) as Vantage `TableSource`s. `AwsAccount` *is* the source; per-operation config lives in the table name (`array_key:service/target`).

- `AwsAccount` with three credential entry points: `from_env()` (standard env vars), `from_credentials_file()` (`[default]` profile in `~/.aws/credentials`; region resolves through `AWS_REGION` → `AWS_DEFAULT_REGION` → `~/.aws/config` `[default]`), and `from_default()` (env first, file fallback). Named profiles, SSO, assume-role, IMDS — out of v0.
- Hand-rolled SigV4 in `src/sign.rs`, no `aws-sdk-*` / `aws-sigv4` dep — just `hmac` / `sha2` / `hex`. Pinned to AWS's canonical-example fixture. Civil-time conversion is hand-rolled too (Hinnant's `days_from_civil`) so we stay off `chrono` / `time` for one function.
- `AwsCondition` with `Eq` (folds into the JSON request body), `In` (literal set; must collapse to a single value at execute time), and `Deferred` (subquery, resolved at execute time). AWS APIs only accept exact-match filters, so multi-value conditions error loudly.
- `AwsOperation` blanket trait so `column.eq(value)` / `column.in_(subquery)` works against any `Expressive<ciborium::Value>`.
- `TableSource` impl: `list_table_values` with deferred-condition resolution, `column_table_values_expr` for relation traversal, `related_in_condition` so `with_one` / `with_many` traversal works without AWS-side joins. Writes (`insert` / `replace` / `patch` / `delete`) and aggregations (`sum` / `min` / `max`) intentionally error.
- Built-in CloudWatch models in `vantage_aws::models`: `log_groups_table` / `log_events_table` (`DescribeLogGroups` / `FilterLogEvents`), with an `events` `with_many` relation between them and a typed `LogGroup::ref_events()` for the entity-in-hand case.
- `examples/aws-cli.rs` — `list-groups`, `list-events`, `traverse`, `--region` override. Output via `vantage_cli_util::print_table`, so the demo exercises the same `Table` / `TableSource` machinery the rest of the framework uses.
- CI workflow (`.github/workflows/aws.yaml`) runs the demo CLI against a live AWS account (`list-groups`, `list-groups --prefix /aws/lambda/`, `traverse`) on every PR — catches signature regressions and wire-format drift the offline tests miss.
