# DynamoDB examples

Two examples in this crate exercise the DynamoDB code path:

- **`aws-dynamo`** — generic walker. Lists every table in the configured account/region and dumps
  each one's contents via `Scan`. Mirrors `aws dynamodb list-tables` + `aws dynamodb scan`.
- **`dynamo-single-table`** — model-driven CLI over a 7-entity single-table design. Demonstrates how
  `PK`/`SK` prefix conventions overlay multiple logical entities on one physical table, and how
  vantage's `Table` conditions + relations express that scoping in Rust.

Either example can run against a local DynamoDB Docker container or against real AWS — the only
difference is whether `AWS_ENDPOINT_URL` is set.

## Local quickstart (DynamoDB Local in Docker)

`scripts/` mirrors the layout used by `vantage-surrealdb/scripts/`. Run from `vantage-aws/`:

```sh
./scripts/start.sh      # docker run amazon/dynamodb-local on :8000
./scripts/ingress.sh    # creates `vantage-demo-single-table` and loads 22 fake items
```

Then point any of the examples at the container:

```sh
export AWS_ENDPOINT_URL=http://localhost:8000
export AWS_ACCESS_KEY_ID=local
export AWS_SECRET_ACCESS_KEY=local
export AWS_REGION=eu-west-2

cargo run --example dynamo-single-table -p vantage-aws -- products
cargo run --example aws-dynamo -p vantage-aws -- --id-field PK
```

`./scripts/stop.sh` tears down the container. DynamoDB Local runs `-inMemory`, so a fresh `start.sh`
always begins empty — re-run `ingress.sh` to repopulate.

DynamoDB Local accepts any `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` values (it doesn't validate
the SigV4 signature), but the SDK still needs _some_ credentials to compute one. The dummies above
are fine.

## Pointing at real AWS

Don't set `AWS_ENDPOINT_URL`. Use any of vantage-aws's normal credential paths:

```sh
# Static creds in env
AWS_ACCESS_KEY_ID=… AWS_SECRET_ACCESS_KEY=… AWS_REGION=eu-west-2 \
    cargo run --example dynamo-single-table -p vantage-aws -- products

# SSO / assume-role profile from ~/.aws/config
AWS_PROFILE=my-sso-profile cargo run --example dynamo-single-table -p vantage-aws -- products
```

The single-table example expects a table named `vantage-demo-single-table` with `PK` (hash) + `SK`
(range), both `S`. You provision it however you like — `terraform`, the AWS console, or copy the
`aws dynamodb create-table` invocation out of `scripts/db/single-table.sh`.

## `dynamo-single-table` CLI grammar

The example is driven by `vantage_cli_util::model_cli`, which gives you a Smalltalk-flavoured
pipeline of _model · filter · index · relation · columns_:

```sh
dynamo-single-table products                          # list all products
dynamo-single-table product[0]                        # first product, single-record view
dynamo-single-table product status=active             # filter (FilterExpression)
dynamo-single-table product[0] :versions              # walk relation: versions of that product
dynamo-single-table product[0] :deployments[0]        # one deployment, deeply
dynamo-single-table products =product_name,status     # override which columns to print
dynamo-single-table teams                             # any of: products, versions, deployments,
                                                      # environments, teams, subscriptions, dataports
```

- `field=value` becomes a DynamoDB `FilterExpression` on the underlying Scan.
- `[N]` selects the Nth row of the current list (and pivots to single-record mode when applied to a list).
- `:relation` walks one of the relations declared on the entity's table factory.
- `=col1,col2,...` (a token starting with `=`) overrides which columns the renderer prints.

Tokens are positional and chain left-to-right: each operates on the result of everything before it.

## Examples

Run against the local fixture. Quote bracketed tokens to keep your shell from globbing —
`'product[1]'` rather than `product[1]`.

```sh
dynamo-single-table 'product[1]' :versions'[1]' :deployments =deployment_id,version_id

dynamo-single-table 'product[0]' :versions'[1]' :deployments =deployment_status,deployment_url

# Singular: filter narrows to one row via find_some, then traverse.
dynamo-single-table product owner_team_id=growth :versions =SK,version

# Plural: filter the list, then `[0]` pivots to the first row before traversing.
dynamo-single-table products 'owner_team_id=growth[0]' :versions =SK,version

dynamo-single-table 'version[0]' :product =SK,product_name

dynamo-single-table deployments =deployment_status,version_id,deployment_url

dynamo-single-table deployments deployment_status=running =SK,version_id,deployment_url

dynamo-single-table 'product[0]' :versions'[1]' :deployments'[2]' :product =product_name,owner_team_id
```

## What the fixture looks like

The fixture loaded by `ingress.sh` puts seven logical entities into one physical table,
distinguished by `PK`/`SK` prefix:

| model          | PK               | SK                                      | count |
| -------------- | ---------------- | --------------------------------------- | ----- |
| `product`      | `METADATA`       | `PRODUCT#<uuid>`                        | 2     |
| `version`      | `PRODUCT#<uuid>` | `VERSION#<v>`                           | 4     |
| `deployment`   | `PRODUCT#<uuid>` | `ENV#<env>#VERSION#<v>#DEPLOYMENT#<id>` | 8     |
| `environment`  | `METADATA`       | `ENV#<name>#<uuid>`                     | 2     |
| `team`         | `METADATA`       | `TEAM#<id>`                             | 2     |
| `subscription` | `SUBSCRIPTION`   | `USER#<email>SUB#<id>`                  | 2     |
| `dataport`     | `DATAPORT`       | `PRODUCT#<uuid>DATASET#<id>`            | 2     |

Each entity's factory in `examples/dynamo-single-table.rs` bakes its scoping conditions in
(`PK = ...` and/or `begins_with(SK, ...)`) and uses `SK` as the row id — so the same `model_cli`
runner that drives `aws-cli` works unchanged.

## Adding your own entity

Mirror the existing factories in `examples/dynamo-single-table.rs`:

1. Define a struct with `#[entity(DynamoType)]` and
   `#[derive(Default, Serialize, Deserialize, ...)]`. Use `Option<...>` for fields — DynamoDB items
   are schemaless, and different rows of the same logical entity may carry different attributes.
2. Add a `dynamo_table(db: DynamoDB) -> Table<DynamoDB, T>` factory that calls
   `with_id_column("SK")`, declares known columns with `with_column_of`, and adds
   `DynamoCondition::eq("PK", ...)` / `DynamoCondition::begins_with("SK", ...)` to scope the scan to
   your entity's PK/SK shape.
3. Register the factory in `ControlApiFactory::for_name` under both singular (`Mode::Single`) and
   plural (`Mode::List`) names.
4. Add a row to `scripts/db/single-table.items.json` matching the new shape.

## Caveats (v0)

- `DynamoId` is partition-key-only. `list_table_values` returns an `IndexMap<DynamoId, Record>`
  keyed by the column you named in `with_id_column` (`SK` here). For per-product entities listed
  _globally_, rows that share an `SK` value collapse — `product[0] :versions` is the right way to
  see all of one product's versions without collisions.
- `Scan + FilterExpression` reads every item DynamoDB has to consider, even those the filter
  rejects. `find_some` (`get_table_some_value`) pages with a moderate per-page chunk and breaks on
  the first match, but a no-match-anywhere walk still costs a full-table scan. Real production code
  should add a GSI and use `Query`; that's planned for v1.
- The single-table example only declares `with_one` / `with_many` relations on the parent's `PK`.
  Sort-key-prefix relations (e.g. only deployments under a specific environment within one product)
  need a `begins_with(SK, ...)` condition on the relation's child table — easy to add, not in the
  demo.
