# vantage-aws

AWS API backend for the [Vantage](https://github.com/romaninsh/vantage)
data framework — incubating.

Treats AWS JSON-1.1 RPC endpoints (CloudWatch Logs, ECS, DynamoDB
control plane, KMS, …) as Vantage `TableSource`s. `AwsAccount` *is* the
source — there's no per-operation wrapper. The operation you want to
run lives in the table name.

## Authentication

Three ways to construct an `AwsAccount`, plus a chain helper:

```rust
use vantage_aws::AwsAccount;

let aws = AwsAccount::new(access_key, secret_key, region);   // explicit
let aws = AwsAccount::from_env()?;                            // standard env vars
let aws = AwsAccount::from_credentials_file()?;               // ~/.aws/credentials [default] only
let aws = AwsAccount::from_default()?;                        // env first, file fallback
```

`from_credentials_file` reads only the `[default]` profile. Region falls
through `AWS_REGION` → `AWS_DEFAULT_REGION` → `~/.aws/config`
`[default]` `region`. `AWS_PROFILE`, SSO, assume-role, IMDS — out of
v0; set the env vars yourself if you need anything fancier.

## Quick start

```rust
use vantage_aws::{AwsAccount, eq};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_dataset::prelude::ReadableValueSet;

let aws = AwsAccount::from_default()?;

let mut groups: Table<AwsAccount, EmptyEntity> = Table::new(
    "logGroups:logs/Logs_20140328.DescribeLogGroups",
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
"logGroups:logs/Logs_20140328.DescribeLogGroups"
    │       │       └── X-Amz-Target header value
    │       └────────── service code (also URL hostname segment)
    └────────────────── response field that holds the row array
```

You only have to write this once per resource — usually you wrap it in
a model factory and forget about the encoding (see below).

## Built-in models

`vantage_aws::models` ships ready-made tables so you don't have to
memorise the table-name format:

- CloudWatch Logs: `log_groups_table`, `log_streams_table`, `log_events_table`.
- ECS (under `models::ecs`): `clusters_table`, `services_table`, `tasks_table`, `task_definitions_table`.

```rust
use vantage_aws::models::{log_groups_table, log_events_table};
use vantage_aws::eq;

let mut groups = log_groups_table(aws.clone());
groups.add_condition(eq("logGroupNamePrefix", "/aws/lambda/"));
let rows = groups.list_values().await?;

let mut events = log_events_table(aws);
events.add_condition(eq("logGroupName", "/aws/lambda/foo"));
let logs = events.list_values().await?;
```

`log_groups_table` registers two `with_many` relations — `events` and
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
cargo run --example aws-cli -- list-groups
cargo run --example aws-cli -- list-groups --prefix /aws/lambda/
cargo run --example aws-cli -- list-events /ecs/ba-nginx
cargo run --example aws-cli -- traverse                        # with_many walk
cargo run --example aws-cli -- --region eu-west-2 list-groups  # override
```

Output goes through `vantage_cli_util::print_table` so it exercises
the same `Table` / `TableSource` machinery the rest of the framework
uses.

## SigV4

No `aws-sdk-*`, no `aws-sigv4` — signing is hand-rolled in `src/sign.rs`
with `hmac` + `sha2` + `hex` and pinned to AWS's canonical-example
fixture. Non-streaming, non-presigned, JSON-1.1 only. If you need
something else, you probably want a different crate.

## Status

v0 covers: `AwsAccount` + JSON-1.1 transport, hand-rolled SigV4,
`Eq` / `In` / `Deferred` conditions, `with_one` / `with_many`
traversal, CloudWatch (`LogGroup`, `LogStream`, `LogEvent`) and ECS
(`Cluster`, `Service`, `Task`, `TaskDefinition`) models, env-var and
`~/.aws/credentials` `[default]` credential loading.

Out of scope for v0:

- **Writes.** `insert_table_value` and friends return errors. Read-only
  end-to-end.
- **Pagination.** First page only. Most JSON-1.1 list operations cap at
  50–100 items per call.
- **Aggregations.** `sum` / `min` / `max` error out — would need a full
  scan.
- **REST-JSON / S3.** Lambda invoke and S3 are different protocols;
  they'll arrive as their own crates.
- **`AWS_PROFILE` / SSO / assume-role / IMDS.** Static credentials and
  the `[default]` profile only.

## License

MIT OR Apache-2.0
