# Changelog

## 0.4.2 — 2026-04-29

AWS Query protocol (form-encoded request, XML response) lands alongside the existing JSON-1.1 transport, plus an IAM submodule that uses it. Same `AwsAccount` is the `TableSource` for both — relations span protocols freely. ([#212](https://github.com/romaninsh/vantage/pull/212))

- The protocol is encoded as a prefix in the table name. Existing tables get `json1/` (e.g. `json1/logGroups:logs/Logs_20140328.DescribeLogGroups`); new IAM tables use `query/` (e.g. `query/Users:iam/2010-05-08.ListUsers`). `AwsAccount::execute_rpc` and `parse_records` match on the prefix and dispatch.
- New `src/query/` mirrors `src/json1/` — `transport.rs` for the signed POST, `mod.rs` with `execute` + `parse_records`. Same hand-rolled SigV4 signer powers both protocols. Responses are XML; `query/xml.rs` normalises them to `serde_json::Value` by stripping `{Action}Response` / `{Action}Result` wrappers and hoisting `<member>` collections into JSON arrays.
- Global services (IAM today, STS later) get a one-line override in `query/transport.rs`: served from `iam.amazonaws.com` (no region in the host) and signed with `us-east-1` regardless of the configured region.
- New `vantage_aws::models::iam` submodule with six top-level tables: `users_table`, `groups_table`, `roles_table`, `policies_table`, `access_keys_table`, `instance_profiles_table`. One `AttachedPolicy` struct shared across `ListAttachedUserPolicies` / `ListAttachedGroupPolicies` / `ListAttachedRolePolicies` (same response shape, different action per source).
- IAM relations on entity factories: `User` → `groups`, `access_keys`, `attached_policies`; `Group` → `attached_policies`; `Role` → `attached_policies`, `instance_profiles`. Both `with_many` traversal and `User::ref_*` / `Group::ref_*` / `Role::ref_*` entity-in-hand forms work — the ref-* form is the right tool for IAM since `ListUsers` / `ListRoles` ignore name filters and return the whole account.
- **Reorganised**: `models::log_*` collapses into `models::logs::*` — `log_groups_table` becomes `logs::groups_table`, etc. Lets the IAM `groups_table` live cleanly under `models::iam::` without a name clash, and matches the `models::ecs::*` shape from 0.4.1. Existing call sites need to update imports.
- `query::parse_records` treats an empty string at the array key as an empty array. `<Foo/>` (self-closing) is what IAM returns for an empty list and the XML normaliser surfaces that as `""` — `list-groups` on an account with no IAM groups now returns "No records found." instead of an obscure decode failure.
- `examples/aws-cli.rs` picks up `list-users`, `list-policies` (`--scope`), `list-roles` (`--path-prefix`), `list-access-keys` (`--user`), `traverse-user-policies`, `traverse-user-access-keys`, `traverse-role-policies`, `traverse-role-profiles`. Log commands renamed to free up `list-groups` for IAM.

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
