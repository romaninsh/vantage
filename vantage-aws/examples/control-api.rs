//! `control-api` — model-driven CLI over the `ddb-odieplat-mercplat-dev-api`
//! single-table design.
//!
//! Same shape as the `aws-cli` example, but instead of multiple physical AWS
//! services it speaks to one DynamoDB table that stores **7 logical entities**
//! distinguished by `PK`/`SK` prefix conventions:
//!
//! | model         | PK                  | SK                                 |
//! |---------------|---------------------|------------------------------------|
//! | `product`     | `METADATA`          | `PRODUCT#<uuid>`                   |
//! | `version`     | `PRODUCT#<uuid>`    | `VERSION#<v>`                      |
//! | `deployment`  | `PRODUCT#<uuid>`    | `ENV#<env>#VERSION#<v>#DEPLOYMENT#<uuid>` |
//! | `environment` | `METADATA`          | `ENV#<name>#<uuid>`                |
//! | `team`        | `METADATA`          | `TEAM#<team_id>`                   |
//! | `subscription`| `SUBSCRIPTION`      | `USER#<email>SUB#<uuid>`           |
//! | `dataport`    | `DATAPORT`          | `PRODUCT#<uuid>DATASET#<id>`       |
//!
//! Each model's table factory bakes its scoping conditions in (`PK = ...`
//! and/or `begins_with(SK, ...)`) and uses `SK` as the row id, so the same
//! model_cli runner that drives `aws-cli` works unchanged.
//!
//! Examples:
//!
//! ```sh
//! export AWS_PROFILE=251736013895_ECPAdmin   # SSO profile; auto-resolved
//! control-api products
//! control-api product id="PRODUCT#381c041d-6f36-40fb-8ecd-bd3c0a3f8397"
//! control-api product[0] :versions
//! control-api product[0] :deployments[0]
//! control-api subscriptions
//! ```
//!
//! v0 caveat: `DynamoId` is partition-key-only, so `list_values` keys items
//! into an `IndexMap<DynamoId, Record>` keyed by `SK`. For per-product
//! entities (Version, Deployment) listed *globally*, rows sharing an SK
//! collapse — e.g. every product's `VERSION#1.0.1` row maps to the same
//! key. Traversing through a parent `product[N] :versions` narrows the
//! scope and the collapse stops mattering.

use anyhow::{Context, Result};
use ciborium::Value as CborValue;
use clap::Parser;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use vantage_aws::AwsAccount;
use vantage_aws::dynamodb::{AnyDynamoType, AttributeValue, DynamoCondition, DynamoDB};
use vantage_cli_util::model_cli::{self, Mode, ModelFactory, Renderer};
use vantage_table::any::AnyTable;
use vantage_table::table::Table;
use vantage_table::traits::table_like::TableLike;
use vantage_types::{Record, entity};

const TABLE: &str = "ddb-odieplat-mercplat-dev-api";

// ── Entity types ─────────────────────────────────────────────────────────
//
// Each `#[entity(DynamoType)]` struct gets `IntoRecord<AnyDynamoType>` /
// `TryFromRecord<AnyDynamoType>` impls, which together with the blanket
// `impl Entity<V> for T` in `vantage-types` satisfies the bound that
// `with_many` / `with_one` need for the build-target closures.
//
// Field types err on the side of `Option<...>` since DynamoDB items are
// schemaless — different rows of the same logical entity can carry
// different attribute sets. We never call `try_from_record` (the
// model_cli runner goes through `AnyTable`'s CBOR bridge), so the
// per-field type only affects how `with_column_of` declares the
// expected column shape.

#[entity(DynamoType)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Product {
    pub product_id: Option<String>,
    pub product_name: Option<String>,
    pub product_type: Option<String>,
    pub status: Option<String>,
    pub owner_team_id: Option<String>,
    pub git_repo_url: Option<String>,
    pub ecr_repo_arn: Option<String>,
    pub active_deployment_count: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[entity(DynamoType)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Version {
    pub version: Option<String>,
    pub product_id: Option<String>,
    pub build_status: Option<String>,
    pub git_commit_hash: Option<String>,
    pub codeartifact_repository: Option<String>,
    pub codeartifact_domain: Option<String>,
    pub s3_bucket: Option<String>,
    pub s3_key: Option<String>,
    pub is_active: Option<bool>,
    pub is_decommissioned: Option<bool>,
    pub changelog: Option<String>,
    pub created_at: Option<String>,
}

#[entity(DynamoType)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Deployment {
    pub deployment_id: Option<String>,
    pub deployment_status: Option<String>,
    pub environment_id: Option<String>,
    pub version_id: Option<String>,
    pub product_id: Option<String>,
    pub is_active: Option<bool>,
    pub is_superseded: Option<bool>,
    pub deployment_url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[entity(DynamoType)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Environment {
    pub environment_id: Option<String>,
    pub name: Option<String>,
    pub aws_account_id: Option<i64>,
    pub aws_region: Option<String>,
    pub approval_required: Option<bool>,
    pub description: Option<String>,
    pub created_at: Option<String>,
}

#[entity(DynamoType)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Team {
    pub team_id: Option<String>,
    pub team_name: Option<String>,
    pub github_url: Option<String>,
    pub created_at: Option<String>,
}

#[entity(DynamoType)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Subscription {
    pub subscription_id: Option<String>,
    pub status: Option<String>,
    pub scope: Option<String>,
    pub requester_user_id: Option<String>,
    pub notify_email: Option<String>,
    pub product_id: Option<String>,
    pub product_name: Option<String>,
    pub version_id: Option<String>,
    pub aws_account_id: Option<String>,
    pub usage: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub created_at: Option<String>,
}

#[entity(DynamoType)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DataPort {
    pub product_id: Option<String>,
    pub dataset_id: Option<String>,
    pub topic: Option<String>,
    pub topic_address: Option<String>,
    pub bootstrap_servers: Option<String>,
    pub r#type: Option<String>,
}

// ── Table factories ──────────────────────────────────────────────────────
//
// All seven entities live in the same physical table; each factory just
// pre-applies the partition/sort-key scoping and declares the columns the
// renderer should know about.

impl Product {
    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, Product> {
        let mut t = Table::<DynamoDB, Product>::new(TABLE, db)
            .with_id_column("SK")
            .with_title_column_of::<Option<String>>("product_name")
            .with_column_of::<Option<String>>("PK")
            .with_column_of::<Option<String>>("product_id")
            .with_column_of::<Option<String>>("product_type")
            .with_column_of::<Option<String>>("status")
            .with_column_of::<Option<String>>("owner_team_id")
            .with_column_of::<Option<String>>("git_repo_url")
            .with_column_of::<Option<i64>>("active_deployment_count")
            .with_column_of::<Option<String>>("created_at")
            .with_many("versions", "PK", Version::dynamo_table)
            .with_many("deployments", "PK", Deployment::dynamo_table);
        t.add_condition(DynamoCondition::eq(
            "PK",
            AttributeValue::S("METADATA".into()),
        ));
        t.add_condition(DynamoCondition::begins_with("SK", "PRODUCT#"));
        t
    }
}

impl Version {
    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, Version> {
        let mut t = Table::<DynamoDB, Version>::new(TABLE, db)
            .with_id_column("SK")
            .with_title_column_of::<Option<String>>("version")
            .with_column_of::<Option<String>>("PK")
            .with_column_of::<Option<String>>("build_status")
            .with_column_of::<Option<String>>("git_commit_hash")
            .with_column_of::<Option<bool>>("is_active")
            .with_column_of::<Option<bool>>("is_decommissioned")
            .with_column_of::<Option<String>>("created_at")
            .with_one("product", "PK", Product::dynamo_table);
        t.add_condition(DynamoCondition::begins_with("SK", "VERSION#"));
        t
    }
}

impl Deployment {
    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, Deployment> {
        let mut t = Table::<DynamoDB, Deployment>::new(TABLE, db)
            .with_id_column("SK")
            .with_title_column_of::<Option<String>>("deployment_id")
            .with_column_of::<Option<String>>("PK")
            .with_column_of::<Option<String>>("environment_id")
            .with_column_of::<Option<String>>("version_id")
            .with_column_of::<Option<String>>("deployment_status")
            .with_column_of::<Option<bool>>("is_active")
            .with_column_of::<Option<bool>>("is_superseded")
            .with_column_of::<Option<String>>("created_at")
            .with_one("product", "PK", Product::dynamo_table);
        t.add_condition(DynamoCondition::begins_with("SK", "ENV#"));
        t
    }
}

impl Environment {
    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, Environment> {
        let mut t = Table::<DynamoDB, Environment>::new(TABLE, db)
            .with_id_column("SK")
            .with_title_column_of::<Option<String>>("name")
            .with_column_of::<Option<String>>("environment_id")
            .with_column_of::<Option<i64>>("aws_account_id")
            .with_column_of::<Option<String>>("aws_region")
            .with_column_of::<Option<bool>>("approval_required")
            .with_column_of::<Option<String>>("description")
            .with_column_of::<Option<String>>("created_at");
        t.add_condition(DynamoCondition::eq(
            "PK",
            AttributeValue::S("METADATA".into()),
        ));
        t.add_condition(DynamoCondition::begins_with("SK", "ENV#"));
        t
    }
}

impl Team {
    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, Team> {
        let mut t = Table::<DynamoDB, Team>::new(TABLE, db)
            .with_id_column("SK")
            .with_title_column_of::<Option<String>>("team_name")
            .with_column_of::<Option<String>>("team_id")
            .with_column_of::<Option<String>>("github_url")
            .with_column_of::<Option<String>>("created_at");
        t.add_condition(DynamoCondition::eq(
            "PK",
            AttributeValue::S("METADATA".into()),
        ));
        t.add_condition(DynamoCondition::begins_with("SK", "TEAM#"));
        t
    }
}

impl Subscription {
    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, Subscription> {
        let mut t = Table::<DynamoDB, Subscription>::new(TABLE, db)
            .with_id_column("SK")
            .with_title_column_of::<Option<String>>("requester_user_id")
            .with_column_of::<Option<String>>("subscription_id")
            .with_column_of::<Option<String>>("status")
            .with_column_of::<Option<String>>("scope")
            .with_column_of::<Option<String>>("notify_email")
            .with_column_of::<Option<String>>("product_id")
            .with_column_of::<Option<String>>("product_name")
            .with_column_of::<Option<String>>("version_id")
            .with_column_of::<Option<String>>("usage")
            .with_column_of::<Option<String>>("start_date")
            .with_column_of::<Option<String>>("end_date");
        t.add_condition(DynamoCondition::eq(
            "PK",
            AttributeValue::S("SUBSCRIPTION".into()),
        ));
        t
    }
}

impl DataPort {
    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, DataPort> {
        let mut t = Table::<DynamoDB, DataPort>::new(TABLE, db)
            .with_id_column("SK")
            .with_title_column_of::<Option<String>>("topic_address")
            .with_column_of::<Option<String>>("product_id")
            .with_column_of::<Option<String>>("dataset_id")
            .with_column_of::<Option<String>>("topic")
            .with_column_of::<Option<String>>("bootstrap_servers")
            .with_column_of::<Option<String>>("type");
        t.add_condition(DynamoCondition::eq(
            "PK",
            AttributeValue::S("DATAPORT".into()),
        ));
        t
    }
}

// ── Factory + renderer for the model_cli runner ─────────────────────────

struct ControlApiFactory(DynamoDB);

impl ModelFactory for ControlApiFactory {
    fn for_name(&self, name: &str) -> Option<(AnyTable, Mode)> {
        let db = self.0.clone();
        let (table, mode): (AnyTable, Mode) = match name {
            "product" => (
                AnyTable::from_table(Product::dynamo_table(db)),
                Mode::Single,
            ),
            "products" => (AnyTable::from_table(Product::dynamo_table(db)), Mode::List),
            "version" => (
                AnyTable::from_table(Version::dynamo_table(db)),
                Mode::Single,
            ),
            "versions" => (AnyTable::from_table(Version::dynamo_table(db)), Mode::List),
            "deployment" => (
                AnyTable::from_table(Deployment::dynamo_table(db)),
                Mode::Single,
            ),
            "deployments" => (
                AnyTable::from_table(Deployment::dynamo_table(db)),
                Mode::List,
            ),
            "environment" => (
                AnyTable::from_table(Environment::dynamo_table(db)),
                Mode::Single,
            ),
            "environments" => (
                AnyTable::from_table(Environment::dynamo_table(db)),
                Mode::List,
            ),
            "team" => (AnyTable::from_table(Team::dynamo_table(db)), Mode::Single),
            "teams" => (AnyTable::from_table(Team::dynamo_table(db)), Mode::List),
            "subscription" => (
                AnyTable::from_table(Subscription::dynamo_table(db)),
                Mode::Single,
            ),
            "subscriptions" => (
                AnyTable::from_table(Subscription::dynamo_table(db)),
                Mode::List,
            ),
            "dataport" => (
                AnyTable::from_table(DataPort::dynamo_table(db)),
                Mode::Single,
            ),
            "dataports" => (AnyTable::from_table(DataPort::dynamo_table(db)), Mode::List),
            _ => return None,
        };
        Some((table, mode))
    }

    fn for_arn(&self, _arn: &str) -> Option<AnyTable> {
        // No ARN syntax for this single-table design.
        None
    }
}

const KNOWN_MODELS: &[&str] = &[
    "product",
    "products",
    "version",
    "versions",
    "deployment",
    "deployments",
    "environment",
    "environments",
    "team",
    "teams",
    "subscription",
    "subscriptions",
    "dataport",
    "dataports",
];

struct CborRenderer;

impl Renderer for CborRenderer {
    fn render_list(
        &self,
        table: &AnyTable,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    ) {
        let id_field = table.id_field_name().unwrap_or_else(|| "SK".to_string());
        let title_fields = table.title_field_names();

        let columns: Vec<String> = if let Some(cols) = column_override {
            cols.iter()
                .map(|c| {
                    if c == "id" {
                        id_field.clone()
                    } else {
                        c.clone()
                    }
                })
                .collect()
        } else if !title_fields.is_empty() {
            title_fields
        } else {
            // Fall back to first three non-key columns from the first record.
            records
                .values()
                .next()
                .map(|rec| {
                    rec.iter()
                        .filter(|(k, _)| k != &"PK" && k != &"SK")
                        .map(|(k, _)| k.clone())
                        .take(3)
                        .collect()
                })
                .unwrap_or_default()
        };

        // Header.
        let mut header = vec![id_field.clone()];
        header.extend(columns.iter().cloned());
        println!("{}", header.join("\t"));

        for (id, rec) in records {
            let mut row = vec![id.clone()];
            for c in &columns {
                row.push(rec.get(c).map(cbor_short).unwrap_or_default());
            }
            println!("{}", row.join("\t"));
        }
        println!(
            "\n({} record{})",
            records.len(),
            if records.len() == 1 { "" } else { "s" }
        );
    }

    fn render_record(
        &self,
        table: &AnyTable,
        id: &str,
        record: &Record<CborValue>,
        relations: &[String],
    ) {
        let id_field = table.id_field_name().unwrap_or_else(|| "SK".to_string());
        println!("{}: {}", id_field, id);
        let title_fields = table.title_field_names();
        for tf in &title_fields {
            if tf == &id_field {
                continue;
            }
            if let Some(v) = record.get(tf) {
                println!("{}: {}", tf, cbor_short(v));
            }
        }
        println!("--------");
        for (k, v) in record.iter() {
            if k == &id_field || title_fields.contains(k) {
                continue;
            }
            println!("{}: {}", k, cbor_short(v));
        }
        if !relations.is_empty() {
            println!();
            println!("Relations:");
            for r in relations {
                println!("  :{r}");
            }
        }
    }
}

fn cbor_short(v: &CborValue) -> String {
    use ciborium::Value as C;
    match v {
        C::Text(s) => s.clone(),
        C::Integer(i) => i128::from(*i).to_string(),
        C::Float(f) => f.to_string(),
        C::Bool(b) => b.to_string(),
        C::Null => "null".to_string(),
        C::Bytes(b) => format!("<{} bytes>", b.len()),
        C::Array(_) | C::Map(_) => cbor_to_json_string(v),
        _ => format!("{v:?}"),
    }
}

/// Serialize a CBOR value to a JSON-style string for compact rendering.
/// Returns `{:?}` if the round-trip fails (rare: CBOR has values JSON
/// can't represent like raw bytes nested in a map, but our DynamoDB
/// records arrived through `attr_to_plain_json` so this won't fire).
fn cbor_to_json_string(v: &CborValue) -> String {
    let mut buf = Vec::new();
    if ciborium::ser::into_writer(v, &mut buf).is_err() {
        return format!("{v:?}");
    }
    let json: serde_json::Value =
        match ciborium::de::from_reader(buf.as_slice()).and_then(|val: CborValue| {
            // re-serialize via serde_json's bridge
            serde_json::to_value(&val)
                .map_err(|_| ciborium::de::Error::Semantic(None, "json conversion failed".into()))
        }) {
            Ok(j) => j,
            Err(_) => return format!("{v:?}"),
        };
    serde_json::to_string(&json).unwrap_or_else(|_| format!("{v:?}"))
}

#[derive(Parser)]
#[command(
    name = "control-api",
    about = "Model-driven CLI over the ddb-odieplat-mercplat-dev-api single-table design",
    long_about = "Walks the seven logical entities (product, version, deployment, environment, team, subscription, dataport) \
                  that share one DynamoDB table. First arg is a model name (singular drops into single-record mode, plural lists). \
                  Chain field=value filters, [N] indices, and :relation traversals after that."
)]
struct Cli {
    /// Override the AWS region (sets AWS_REGION before credential load).
    #[arg(long, global = true, default_value = "eu-west-1")]
    region: String,

    /// Positional tokens: model, filters, indices, traversals.
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    args: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // SAFETY: single-threaded, before any other env reads.
    unsafe { std::env::set_var("AWS_REGION", &cli.region) };

    let aws = AwsAccount::from_default().context(
        "Set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/AWS_REGION, or configure ~/.aws/credentials [default]",
    )?;

    if cli.args.is_empty() {
        eprintln!(
            "usage: control-api [--region REGION] <model> [field=value ...] [[N]] [:relation ...]"
        );
        eprintln!("\nKnown models:");
        for n in KNOWN_MODELS {
            eprintln!("  {n}");
        }
        std::process::exit(2);
    }

    let factory = ControlApiFactory(DynamoDB::new(aws));
    let renderer = CborRenderer;
    model_cli::run(&factory, &renderer, &cli.args).await?;
    Ok(())
}
