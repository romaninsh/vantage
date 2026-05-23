//! `vantage-aws-cli` — Vista-driven CLI over `vantage-aws`.
//!
//! Argument grammar matches [`vantage_cli_util::vista_cli`] — see that
//! crate's docs for the full vocabulary (operators, sort/slice
//! selectors, search, aggregates, locators, JSON-typed values). Common
//! shapes:
//!
//! ```text
//! aws-cli [--region <r>] [--format=<f>] <model | arn> [token …]
//! ```
//!
//! - First positional is a dotted model name (`iam.users`, `log.group`,
//!   `ecs.task_definitions`, …) or an ARN. Singular names render the
//!   first record; plurals render a list.
//! - `field=value` / `field:lt=value` / `field="quoted text"` filter.
//! - `[N]` narrows to row N; `[+name]` sorts ascending; `[+name:0]`
//!   sorts then narrows.
//! - `:relation` traverses a `with_many` / `with_one`.
//! - `=col1,col2` overrides the rendered columns.
//! - `?keyword` searches; `@count` / `@sum:field` aggregate.
//! - `--format=table|json|ndjson|cbor-diag` switches output.
//!
//! Reads creds from the standard env vars (`AWS_ACCESS_KEY_ID`,
//! `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`,
//! `AWS_REGION`), falling back to the `[default]` profile in
//! `~/.aws/credentials` and `~/.aws/config`.

use anyhow::{Context, Result};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_aws::AwsAccount;
use vantage_aws::models::{Factory, FactoryMode};
use vantage_aws::types::AnyAwsType;
use vantage_cli_util::output::{self, OutputFormat};
use vantage_cli_util::vista_cli::{self, AggregateOp, Mode, ModelFactory, Renderer};
use vantage_cli_util::{render_records_columns, render_records_typed};
use vantage_types::{Record, TerminalRender};
use vantage_vista::{ReferenceKind, Vista};

/// Adapts [`Factory`] (which lives in `vantage-aws` proper, free of any
/// CLI-runner dep) to `vantage-cli-util`'s vista_cli factory trait.
struct AwsFactoryAdapter(Factory);

impl ModelFactory for AwsFactoryAdapter {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        self.0.for_name(name).map(|(v, m)| {
            (
                v,
                match m {
                    FactoryMode::List => Mode::List,
                    FactoryMode::Single => Mode::Single,
                },
            )
        })
    }

    fn for_locator(&self, locator: &str) -> Option<Vista> {
        self.0.from_arn(locator)
    }
}

/// AWS-flavoured renderer. For `--format=table`, lists go through
/// `render_records_typed` / `render_records_columns` with values typed
/// up via `AnyAwsType` so ARNs / timestamps render as themselves;
/// single records print `id` + title columns, `--------`, then the
/// rest, followed by traversable relations. Other formats forward to
/// `vantage_cli_util::output`.
struct AwsRenderer {
    format: OutputFormat,
}

impl Renderer for AwsRenderer {
    fn render_list(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    ) {
        match self.format {
            OutputFormat::Table => render_list_table(vista, records, column_override),
            _ => print!("{}", output::render_list(self.format, records)),
        }
    }

    fn render_record(
        &self,
        vista: &Vista,
        id: &str,
        record: &Record<CborValue>,
        _relations: &[String],
    ) {
        match self.format {
            OutputFormat::Table => render_record_table(vista, id, record),
            _ => print!("{}", output::render_record(self.format, id, record)),
        }
    }

    fn render_scalar(
        &self,
        _vista: &Vista,
        op: AggregateOp,
        field: Option<&str>,
        value: &CborValue,
    ) {
        let label = match field {
            Some(f) => format!("{}({f})", op.name()),
            None => format!("{}()", op.name()),
        };
        match self.format {
            OutputFormat::Table => println!("{label} = {}", scalar(value)),
            _ => print!("{}", output::render_scalar(self.format, &label, value)),
        }
    }
}

fn render_list_table(
    vista: &Vista,
    records: &IndexMap<String, Record<CborValue>>,
    column_override: Option<&[String]>,
) {
    let id_field = vista.get_id_column().map(str::to_string);
    let title_fields: Vec<String> = vista
        .get_title_columns()
        .into_iter()
        .map(str::to_string)
        .collect();
    let column_types = column_types_static(vista);

    let typed = typed_records_borrowed(records, vista);

    if let Some(cols) = column_override {
        // `id` in an override resolves to the table's id field.
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

    // Default: id + title columns. Tables without title columns
    // (single-column ARN tables in ECS) fall back to every non-id
    // column so the listing isn't empty.
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

fn render_record_table(vista: &Vista, id: &str, record: &Record<CborValue>) {
    let id_field = vista.get_id_column().map(str::to_string);
    let title_fields: Vec<String> = vista
        .get_title_columns()
        .into_iter()
        .map(str::to_string)
        .collect();

    let typed_rec: Record<AnyAwsType> = record
        .iter()
        .map(|(k, v)| {
            let declared = vista
                .get_column(k)
                .map(|c| c.original_type.as_str())
                .unwrap_or("");
            (k.clone(), AnyAwsType::from_cbor_typed(v.clone(), declared))
        })
        .collect();

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

    for name in vista.get_column_names() {
        if Some(name) == id_field.as_deref() || title_fields.iter().any(|t| t == name) {
            continue;
        }
        if let Some(s) = format_field(&typed_rec, name) {
            println!("{name}: {s}");
        }
    }

    let refs = vista.list_references();
    if !refs.is_empty() {
        println!();
        println!("Relations:");
        for (name, kind) in refs {
            let marker = match kind {
                ReferenceKind::HasOne => "→ one",
                ReferenceKind::HasMany => "↠ many",
            };
            println!("  :{name}  {marker}");
        }
    }
}

/// Materialise vista's column metadata as `IndexMap<name, &'static str>`
/// for the `render_records_*` helpers. Vista stores type names as
/// `String`; the helpers expect `&'static str` (from `Column::get_type()`).
/// We leak each string once — the AWS column-type set is bounded by
/// the number of distinct Rust types across registered models, and the
/// CLI exits shortly after rendering.
fn column_types_static(vista: &Vista) -> IndexMap<String, &'static str> {
    let mut out = IndexMap::new();
    for name in vista.get_column_names() {
        let original = vista
            .get_column(name)
            .map(|c| c.original_type.as_str())
            .unwrap_or("");
        let leaked: &'static str = Box::leak(original.to_owned().into_boxed_str());
        out.insert(name.to_string(), leaked);
    }
    out
}

fn typed_records_borrowed(
    records: &IndexMap<String, Record<CborValue>>,
    vista: &Vista,
) -> IndexMap<String, Record<AnyAwsType>> {
    records
        .iter()
        .map(|(id, record)| {
            let typed: Record<AnyAwsType> = record
                .iter()
                .map(|(k, v)| {
                    let declared = vista
                        .get_column(k)
                        .map(|c| c.original_type.as_str())
                        .unwrap_or("");
                    (k.clone(), AnyAwsType::from_cbor_typed(v.clone(), declared))
                })
                .collect();
            (id.clone(), typed)
        })
        .collect()
}

fn format_field(record: &Record<AnyAwsType>, key: &str) -> Option<String> {
    record.get(key).map(|v| v.render().to_string())
}

fn scalar(v: &CborValue) -> String {
    match v {
        CborValue::Text(s) => s.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "—".to_string(),
        CborValue::Bytes(b) => format!("<{} bytes>", b.len()),
        other => format!("{other:?}"),
    }
}

fn print_usage() {
    eprintln!(
        "usage: aws-cli [--region <r>] [--format=<f>] <model | arn> [field=value ...] [[N]] [:relation ...] [@op[:field]]"
    );
    eprintln!("\nFormats: table (default), json, ndjson, cbor-diag");
    eprintln!("\nKnown models:");
    for name in Factory::known_names() {
        eprintln!("  {name}");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Strip `--region <r>` / `--format=<f>` out of argv before forwarding
    // positionals to vista_cli. `--region` is AWS-specific (sets the env
    // var before credential load); `--format=…` mirrors `cli-vista.rs`.
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut region: Option<String> = None;
    let mut format = OutputFormat::Table;
    let mut positional: Vec<String> = Vec::with_capacity(raw.len());

    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        if let Some(value) = arg.strip_prefix("--format=") {
            format = OutputFormat::parse(value)
                .with_context(|| format!("unknown --format `{value}`"))?;
        } else if arg == "--region" {
            region = it.next();
        } else if let Some(value) = arg.strip_prefix("--region=") {
            region = Some(value.to_string());
        } else {
            positional.push(arg);
        }
    }

    if let Some(r) = &region {
        // SAFETY: single-threaded, before any other env reads.
        unsafe { std::env::set_var("AWS_REGION", r) };
    }

    if positional.is_empty() {
        print_usage();
        std::process::exit(2);
    }

    let aws = AwsAccount::from_default().context(
        "Set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/AWS_REGION, or configure ~/.aws/credentials [default]",
    )?;

    let factory = AwsFactoryAdapter(Factory::new(aws));
    let renderer = AwsRenderer { format };
    vista_cli::run(&factory, &renderer, &positional).await?;
    Ok(())
}
