//! `vantage-aws-cli` — generic, model-driven CLI over `vantage-aws`.
//!
//! Argument shape (everything after `--region`):
//!
//! ```text
//! aws-cli <model | arn> [field=value ...] [[N]] [:relation [[N]] ...]
//! ```
//!
//! - First positional is either a dotted model name (`iam.users`,
//!   `log.group`, `ecs.task_definitions`, …) or an ARN (`arn:...`).
//! - Singular forms (`iam.user`) drop into single-record mode and
//!   render the first matching record. Plural forms (`iam.users`)
//!   render a list.
//! - Filters (`field=value` or `field="quoted value"`) AND together.
//! - `[N]` selects the Nth record from a list and switches into
//!   single-record mode.
//! - `:relation` traverses a `with_many` / `with_one` registered on
//!   the current table and switches into list mode for the child.
//! - Glued forms work too: `users[0]`, `:members[0]`, `name=foo[0]`.
//!
//! Reads creds from the standard env vars (`AWS_ACCESS_KEY_ID`,
//! `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`,
//! `AWS_REGION`), falling back to the `[default]` profile in
//! `~/.aws/credentials` and `~/.aws/config`.

use anyhow::{Context, Result};
use ciborium::Value as CborValue;
use clap::Parser;
use indexmap::IndexMap;
use vantage_aws::AwsAccount;
use vantage_aws::models::{Factory, FactoryMode};
use vantage_aws::types::{AnyAwsType, typed_records};
use vantage_cli_util::model_cli::{self, Mode, ModelFactory, Renderer};
use vantage_cli_util::{render_records_columns, render_records_typed};
use vantage_table::any::AnyTable;
use vantage_table::traits::table_like::TableLike;
use vantage_types::{Record, TerminalRender};

#[derive(Parser)]
#[command(
    name = "vantage-aws-cli",
    about = "Generic CLI for vantage-aws models",
    long_about = "Use a dotted model name (e.g. iam.users, log.group) or an ARN as the first arg, \
                  then chain filters (field=value), index selectors ([0]), and relation \
                  traversals (:relation). Example: aws-cli iam.user UserName=alice :groups."
)]
struct Cli {
    /// Override the AWS region (sets AWS_REGION before credential load).
    #[arg(long, global = true)]
    region: Option<String>,

    /// Positional tokens: model-or-ARN, filters, indices, traversals.
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    args: Vec<String>,
}

/// Adapts the standalone [`Factory`] (which lives in `vantage-aws`
/// proper) to `vantage-cli-util`'s [`ModelFactory`] trait. Keeps
/// `vantage-aws` itself free of a runtime dep on `vantage-cli-util`.
struct AwsFactoryAdapter(Factory);

impl ModelFactory for AwsFactoryAdapter {
    fn for_name(&self, name: &str) -> Option<(AnyTable, Mode)> {
        self.0.for_name(name).map(|(t, m)| {
            (
                t,
                match m {
                    FactoryMode::List => Mode::List,
                    FactoryMode::Single => Mode::Single,
                },
            )
        })
    }

    fn for_arn(&self, arn: &str) -> Option<AnyTable> {
        self.0.from_arn(arn)
    }
}

/// AWS-aware renderer. Lists go through the existing typed table
/// renderer (with non-title columns hidden); single records print
/// `id` + title columns, then `----`, then the rest, and finally a
/// list of traversable relations.
struct AwsRenderer;

impl Renderer for AwsRenderer {
    fn render_list(
        &self,
        table: &AnyTable,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    ) {
        let id_field = table.id_field_name();
        let column_types = table.column_types();
        let title_fields = table.title_field_names();

        let typed = typed_records(records.clone(), &column_types);

        if let Some(cols) = column_override {
            // Explicit override: only the spelled-out columns are
            // shown. `id` resolves to the table's id field.
            let resolved: Vec<String> = cols
                .iter()
                .map(|raw| {
                    if raw == "id" {
                        id_field.clone().unwrap_or_else(|| raw.clone())
                    } else {
                        raw.clone()
                    }
                })
                .collect();
            render_records_columns(&typed, &resolved, &column_types);
            return;
        }

        // Default: show id + title columns. Tables with no title
        // columns (single-column ARN tables in ECS) fall back to
        // every non-id column so the listing isn't empty.
        let visible: IndexMap<String, &'static str> = if title_fields.is_empty() {
            column_types.clone()
        } else {
            let mut v = IndexMap::new();
            for f in &title_fields {
                if let Some(t) = column_types.get(f) {
                    v.insert(f.clone(), *t);
                }
            }
            v
        };
        render_records_typed(&typed, id_field.as_deref(), &visible);
    }

    fn render_record(
        &self,
        table: &AnyTable,
        id: &str,
        record: &Record<CborValue>,
        relations: &[String],
    ) {
        let id_field = table.id_field_name();
        let title_fields = table.title_field_names();
        let column_types = table.column_types();

        let typed_rec: Record<AnyAwsType> = record
            .iter()
            .map(|(k, v)| {
                let declared = column_types.get(k).copied().unwrap_or("");
                (k.clone(), AnyAwsType::from_cbor_typed(v.clone(), declared))
            })
            .collect();

        // id leads, then title columns, then `--------`, then the rest.
        if let Some(ref name) = id_field {
            println!(
                "{}: {}",
                name,
                format_field(&typed_rec, name).unwrap_or_else(|| id.to_string())
            );
        } else {
            println!("id: {id}");
        }
        for tf in &title_fields {
            if Some(tf.as_str()) == id_field.as_deref() {
                continue;
            }
            if let Some(s) = format_field(&typed_rec, tf) {
                println!("{tf}: {s}");
            }
        }

        println!("--------");

        for k in column_types.keys() {
            if Some(k.as_str()) == id_field.as_deref() || title_fields.contains(k) {
                continue;
            }
            if let Some(s) = format_field(&typed_rec, k) {
                println!("{k}: {s}");
            }
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

fn format_field(record: &Record<AnyAwsType>, key: &str) -> Option<String> {
    record.get(key).map(|v| v.render().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(region) = &cli.region {
        // SAFETY: single-threaded, before any other env reads.
        unsafe { std::env::set_var("AWS_REGION", region) };
    }
    let aws = AwsAccount::from_default().context(
        "Set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/AWS_REGION, or configure ~/.aws/credentials [default]",
    )?;

    if cli.args.is_empty() {
        eprintln!("usage: aws-cli <model | arn> [field=value ...] [[N]] [:relation ...]");
        eprintln!("\nKnown models:");
        for name in Factory::known_names() {
            eprintln!("  {name}");
        }
        std::process::exit(2);
    }

    let factory = AwsFactoryAdapter(Factory::new(aws));
    let renderer = AwsRenderer;
    model_cli::run(&factory, &renderer, &cli.args).await?;
    Ok(())
}
