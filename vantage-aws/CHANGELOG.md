# Changelog

## 0.4.5 — 2026-05-04

A typed DynamoDB persistence backend — sibling to the existing list-API surface in `models::dynamodb`. Items have a typed `AttributeValue` representation, native key/filter expressions, and full CRUD semantics, none of which fold cleanly into the `AwsAccount`-as-`TableSource` shape — so DynamoDB lives as its own [`vantage_aws::dynamodb`](https://docs.rs/vantage-aws/0.4.5/vantage_aws/dynamodb/) module with its own `TableSource` impl.

- New [`DynamoDB`](https://docs.rs/vantage-aws/0.4.5/vantage_aws/dynamodb/struct.DynamoDB.html) data source: cheap to clone, wraps an existing `AwsAccount` for credentials and signing.
- New [`AttributeValue`](https://docs.rs/vantage-aws/0.4.5/vantage_aws/dynamodb/enum.AttributeValue.html) typed enum mirrors the wire shape (`S`, `N`, `B`, `BOOL`, `NULL`, `L`, `M`, `SS`, `NS`, `BS`); a `vantage_type_system!` invocation produces the matching `DynamoType` trait, `AnyDynamoType` value, and variants enum, with per-type impls for `String`, `i32`, `i64`, `f64`, `bool`, `Vec<u8>`, and `Option<T>`.
- Read/write path covers `Scan` (list / count / sample), `GetItem` (point read), `PutItem` (insert and replace), and `DeleteItem`. `delete_table_all_values` walks the scan and deletes per-item — no native bulk delete.
- New [`DynamoCondition`](https://docs.rs/vantage-aws/0.4.5/vantage_aws/dynamodb/enum.DynamoCondition.html) carries a rendered expression plus its `ExpressionAttributeNames` / `ExpressionAttributeValues` maps; v0 ships `eq` only, with `gt` / `between` / `in_` / `begins_with` landing alongside Scan/Query filter execution.
- New [`DynamoId`](https://docs.rs/vantage-aws/0.4.5/vantage_aws/dynamodb/struct.DynamoId.html) primary-key type — partition-key-only and string-form in v0; composite (partition + sort) keys are next.
- New `AwsAccount::with_region(region)` returns a copy with the region overridden — useful when credentials come from `~/.aws/credentials` but the target region differs from the profile default (e.g. a test fixture provisioned in a fixed region regardless of the developer's local config).
- Added `paste` dependency (used by the `vantage_type_system!` macro expansion).
- Live integration tests against a real DynamoDB account (`tests/dynamodb_live.rs`); auto-skip when AWS credentials aren't configured. The accompanying `test-tf/dynamodb.tf` provisions two `PAY_PER_REQUEST` tables (`<name>-products`, `<name>-orders`) for exercising the round-trip.
- Aggregations (`sum` / `min` / `max`), `patch` (UpdateItem), key-generation insert, relationship traversal, and binary `B` / `BS` wire codec are stubbed — they error loudly so callers can't accidentally rely on them.

## 0.4.4 — 2026-05-02

Three more AWS wire protocols and three more model trees — S3, Lambda, DynamoDB — wired into the same `Factory` / `from_arn` surface as the IAM/ECS/CloudWatch models. ([#216](https://github.com/romaninsh/vantage/pull/216))

- New `restxml/` protocol prefix, used by S3. Target syntax is `restxml/<array_key>:<service>/<METHOD> <path>?<static-query>` — conditions whose name matches a `{Placeholder}` segment fill the path; everything else appends to the query string. The transport adds `x-amz-content-sha256` to the signed headers (mandatory on S3, harmless elsewhere); the response parser strips the XML root and supports dotted lookup so callers can target nested arrays like `Buckets.Bucket`.
- New `restjson/` protocol prefix, used by Lambda. Same request shape as `restxml/` (reuses the path-template builder) but parses a JSON response. No content-sha256 header — Lambda doesn't require it.
- New `json10/` protocol prefix, used by DynamoDB. Differs from `json1/` only in the `application/x-amz-json-1.0` content type, so it shares the JSON-1.1 transport via a new `json_aws_call(..., content_type)` helper. Parse path is identical to `json1/`.
- New `vantage_aws::models::s3` submodule: `buckets_table` (`ListBuckets`) + `objects_table` (`ListObjectsV2`, requires `Bucket`). `Bucket` carries an `objects` `with_many` relation. Path-style addressing only — cross-region buckets surface AWS's 301 verbatim with the home-region endpoint in the error body, so the caller can re-point `AwsAccount`'s region.
- New `vantage_aws::models::lambda` submodule: `functions_table` (`ListFunctions`) + `aliases_table` / `versions_table` (per function). `Function` carries `aliases` and `versions` `with_many`s plus a cross-service `log_group` `with_foreign` that resolves to the matching CloudWatch group at `/aws/lambda/<FunctionName>` via a deferred expression — projects `FunctionName` from the source row, prefixes it, and hands the result to `logGroupNamePrefix` for AWS-side narrowing.
- New `vantage_aws::models::dynamodb` submodule: `tables_table` (`ListTables`). The list-side response is just an array of strings, so v0 surfaces `TableName` only; richer `DescribeTable` metadata is a follow-up.
- `Factory::known_names` picks up six new entries: `s3.bucket(s)`, `lambda.function(s)`, `dynamodb.table(s)`. Sub-resources (`s3.object`, `lambda.alias`, `lambda.version`) intentionally aren't exposed top-level — listing them standalone needs a parent filter, so they're reachable only via traversal. `Factory::from_arn` learns four new ARN shapes (S3 object, S3 bucket, Lambda function, DynamoDB table).
- `dispatch::lookup_path` — small helper that walks dotted paths through `serde_json::Value`. Adopted by `json1::parse_records` and the new REST parsers so the same `array_key` syntax works across protocols.

## 0.4.3 — 2026-04-30

`vantage-aws` picks up a generic, type-erased model factory and the AWS-side machinery to back the new model-driven CLI in `vantage-cli-util`. ([#215](https://github.com/romaninsh/vantage/pull/215))

- New `vantage_aws::models::Factory` — dotted-string lookup (`iam.user`, `log.group`, `ecs.cluster`, …) to `AnyTable` plus a single `Factory::from_arn` entry point. Singular forms drop into `FactoryMode::Single`, plural into `FactoryMode::List`. Models whose AWS API requires a parent filter aren't exposed top-level; they're reachable via traversal:
  - `iam.user … :access_keys`        (`ListAccessKeys` needs `UserName`)
  - `log.group … :streams`           (`DescribeLogStreams` needs `logGroupName`)
  - `log.group … :events`            (`FilterLogEvents` needs `logGroupName`)
  - `ecs.cluster … :services`        (`ListServices` needs `cluster`)
  - `ecs.cluster … :tasks`           (`ListTasks` needs `cluster`)
- New per-entity `from_arn(arn, aws) -> Option<Table<…>>` on `User`, `Group`, `Role`, `Policy`, `InstanceProfile`, `AccessKey`, `LogGroup`, `LogStream`, `Cluster`. Each parses its own ARN shape and returns a pre-conditioned table. `Factory::from_arn` walks them in order.
- `ecs::clusters_table` gains the previously-missing `with_many("services", "cluster", services_table)` and `with_many("tasks", "cluster", tasks_table)` so cluster traversal hits the registered relations.
- Existing IAM / Logs models pick up `with_title_column_of` for the columns worth showing in lists (`Path`, `CreateDate`, `UserName`, `Status`, `PolicyName`, `IsAttachable`, …). Long, noisy, or always-empty columns (`Arn`, `AssumeRolePolicyDocument`, `PasswordLastUsed`, log message body) stay hidden by default and surface when you drill into a single record.
- New `TableSource::eq_condition` impl on `AwsAccount` — builds an `AwsCondition::Eq` from raw strings so the generic CLI's `add_condition_eq` works without reaching into the backend's expression type.
- `AwsAccount::list_table_values` now post-filters records by any `Eq` condition whose field appears on the records. AWS APIs only push down their own request-param filters (`PathPrefix`, `logGroupNamePrefix`, `cluster`); eq conditions on actual record fields (`UserName`, `Path`, `clusterArn`) used to be silently dropped on the wire. Fields not on any record are assumed to be request params and skipped (no over-filtering on `PathPrefix` etc.). Also makes the deferred-subquery path under `:relation` resolve correctly when the source is narrowed in-memory.
- `examples/aws-cli.rs` rewritten as a thin adapter around `vantage_cli_util::model_cli::run` — same end-to-end paths as before plus filters / index / column-overrides / ARN-as-first-argument. The previous clap subcommand surface is gone; everything goes through the generic argv parser.
- Pins `vantage-table = "0.4.8"` for the new `TableLike` reflection methods + `with_title_column_of`.

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
