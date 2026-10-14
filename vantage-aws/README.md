# vantage-aws

AWS API backend for the [Vantage](https://github.com/romaninsh/vantage)
data framework — incubating.

Treats AWS API endpoints as Vantage `TableSource`s. `AwsAccount` *is*
the source — there's no per-operation wrapper. The operation you want
to run lives in the table name.

Five wire protocols ship today: **JSON-1.1** (`json1`: CloudWatch, ECS,
KMS, …), **JSON-1.0** (`json10`: DynamoDB), **Query** (`query`: IAM,
STS, EC2, ELBv1, SES, …), **REST-XML** (`restxml`: S3) and **REST-JSON**
(`restjson`: Lambda). The protocol is picked from the table name's
prefix, so the same `AwsAccount` covers everything and relations span
services freely.

DynamoDB items also have their own `TableSource` in
`vantage_aws::dynamodb` (`DynamoDB::new(aws)`), with typed attribute
values and writes. See [README-DYNAMODB.md](README-DYNAMODB.md).

## Authentication

Three ways to construct an `AwsAccount`, plus a chain helper:

```rust
use vantage_aws::AwsAccount;

let aws = AwsAccount::new(access_key, secret_key, region);   // explicit
let aws = AwsAccount::from_env()?;                            // standard env vars
let aws = AwsAccount::from_credentials_file()?;               // profile from AWS_PROFILE, or [default]
let aws = AwsAccount::from_profile("dev")?;                   // a named profile
let aws = AwsAccount::from_default()?;                        // env first, file fallback
```

`from_credentials_file` reads the profile named by `AWS_PROFILE`
(`default` when unset) from `~/.aws/credentials`. For SSO, assume-role
or `credential_process` profiles whose keys aren't in that file, it
falls back to `aws configure export-credentials`, so the AWS CLI must
be installed (and `aws sso login` must have run for SSO). Region falls
through `AWS_REGION` → `AWS_DEFAULT_REGION` → the profile's `region` in
`~/.aws/config`. IMDS is not supported.

`AwsAccount::public(region)` sends unsigned requests, for resources that
allow anonymous access such as public S3 buckets. Every constructor
honours `AWS_ENDPOINT_URL`; `with_endpoint` and `with_region` override
per account.

## Quick start

```rust
use vantage_aws::{AwsAccount, eq};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_dataset::prelude::ReadableValueSet;

let aws = AwsAccount::from_default()?;

let mut groups: Table<AwsAccount, EmptyEntity> = Table::new(
    "json1/logGroups:logs/Logs_20140328.DescribeLogGroups",
    aws,
);
groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));

let rows = groups.list_values().await?;
```

That's a CloudWatch `DescribeLogGroups` call, filtered by prefix. The
condition folds into the JSON request body; the response array gets
parsed into `Record<CborValue>` rows.

## Anatomy of a table name

```text
"json1/logGroups:logs/Logs_20140328.DescribeLogGroups"
   │       │      │       └── X-Amz-Target header value
   │       │      └────────── service code (also URL hostname segment)
   │       └───────────────── response field that holds the row array
   └───────────────────────── wire protocol (json1, json10, query, restxml, restjson)

"query/Users:iam/2010-05-08.ListUsers"
   │     │    │     │
   │     │    │     └── "VERSION.Action" — both go into the form body
   │     │    └──────── service code (also URL hostname segment)
   │     └───────────── response element name (Query lists wrap in <member>)
   └─────────────────── wire protocol
```

A paginated operation adds its cursor to the array key: `@nextToken`
when the request and response fields share a name,
`@request=response` when they differ
(`restxml/Contents@continuation-token=NextContinuationToken:s3/GET /{Bucket}?list-type=2`).
REST tables put `METHOD path` in the target, and `{Placeholder}`
segments are filled from conditions.

You only have to write this once per resource — usually you wrap it in
a model factory and forget about the encoding (see below).

## Built-in models

`vantage_aws::models` ships ready-made tables so you don't have to
memorise the table-name format:

- CloudWatch Logs (under `models::logs`): `groups_table`, `streams_table`, `events_table`.
- ECS (under `models::ecs`): `clusters_table`, `services_table`, `tasks_table`, `task_definitions_table`.
- IAM (under `models::iam`): `users_table`, `groups_table`, `roles_table`, `policies_table`, `access_keys_table`, `instance_profiles_table`, and the `attached_*_policies_table` family.
- Lambda (under `models::lambda`): `functions_table`, `aliases_table`, `versions_table`.
- S3 (under `models::s3`): `buckets_table`, `objects_table`, plus `get_object`.
- DynamoDB (under `models::dynamodb`): `tables_table`.

```rust
use vantage_aws::models::logs::{groups_table, events_table};
use vantage_aws::eq;

let mut groups = groups_table(aws.clone());
groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
let rows = groups.list_values().await?;

let mut events = events_table(aws);
events.add_condition(eq("logGroupName", "/aws/lambda/foo"));
let logs = events.list_values().await?;
```

`logs::groups_table` registers two `with_many` relations — `events` and
`streams` — that traverse to the group's log events / streams. AWS
doesn't accept multi-value filters, so the source has to narrow to a
single group before traversal — otherwise the call errors at execute
time.

ECS APIs split into a `List*` step (returns ARNs only) and a `Describe*`
step (full objects). v0 exposes the `List*` side; rows hold the ARN
plus parsed-out short-name helpers (`Cluster::name()`,
`Task::task_id()`, etc.).

## Conditions

`eq` folds straight into the JSON request body. AWS APIs only accept
exact-match filters, so that's all you really get:

```rust
use vantage_aws::eq;

table.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
```

Bring `AwsOperation` into scope to write `column.eq(...)` instead:

```rust
use vantage_aws::AwsOperation;

table.add_condition(table["logGroupName"].clone().eq("/ecs/ba-nginx"));
```

`In` and `Deferred` are here to make `with_one` / `with_many`
traversal work — they must collapse to a single value at execute
time, otherwise the call errors loudly.

## Demo CLI

`examples/aws-cli.rs` exercises the end-to-end machinery:

```sh
cargo run --example aws-cli -- log.groups
cargo run --example aws-cli -- log.groups logGroupNamePrefix=/aws/lambda/
cargo run --example aws-cli -- log.group logGroupNamePrefix=/ecs/ba-nginx :events
cargo run --example aws-cli -- iam.users
cargo run --example aws-cli -- --region eu-west-2 log.groups
```

The first argument is a model name (`iam.users`, `log.group`,
`ecs.task_definitions`, `s3.buckets`, `lambda.functions`, …) or an ARN.
Singular names render the first record, plurals a list, and `:relation`
traverses. The grammar comes from `vantage_cli_util::vista_cli`, so it
exercises the same `Vista` machinery the rest of the framework uses.
`examples/aws-dynamo.rs` is the DynamoDB counterpart (see
README-DYNAMODB.md).

## SigV4

No `aws-sdk-*`, no `aws-sigv4` — signing is hand-rolled in `src/sign.rs`
with `hmac` + `sha2` + `hex` and pinned to AWS's canonical-example
fixture. Non-streaming, non-presigned. The same signer powers every
transport (JSON, form-encoded Query, REST-XML and REST-JSON).

## Status

v0 covers: `AwsAccount` + JSON-1.1, JSON-1.0, Query, REST-XML and
REST-JSON transports, hand-rolled SigV4, `Eq` / `In` / `Deferred`
conditions, `with_one` / `with_many` traversal, CloudWatch Logs, ECS,
IAM, Lambda, S3 and DynamoDB models, a `Vista` bridge
(`aws.vista_factory().from_table(table)?`: read-only, count, plus
`fetch_next` on S3 listings; no YAML specs),
and env-var, profile and AWS CLI credential loading.

Pagination: JSON-1.x and REST-XML operations whose table name carries a
cursor walk every page (CloudWatch Logs and ECS via `nextToken`, S3
`ListObjectsV2` via its continuation token). Cap the walk with
`AwsAccount::with_max_pages(n)`. Query (IAM) and REST-JSON (Lambda)
operations return the first page only.

Out of scope for v0:

- **Writes.** On the `AwsAccount` source, `insert_table_value` and
  friends return errors. Read-only end-to-end. (`vantage_aws::dynamodb`
  is the exception.)
- **Aggregations.** `sum` / `min` / `max` error out — would need a full
  scan.
- **IMDS.** Instance-metadata credentials are not read.

## License

MIT OR Apache-2.0
