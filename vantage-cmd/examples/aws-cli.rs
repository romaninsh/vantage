//! `aws-cli` — a Vista-driven CLI over `vantage-cmd`, wrapping the real
//! `aws` CLI. Modeled on `bakery_model3/examples/cli-vista.rs`: a thin
//! [`ModelFactory`] resolves dotted model names to YAML vistas, and the
//! generic [`vista_cli`] runner does the rest. Traversal is built-in —
//! references declared in each vista's YAML are lowered onto the table, so
//! `:relation` tokens "just work" with no example-side code.
//!
//! ```text
//! aws-cli [--region <r>] [--format=<f>] <model> [field=value …] [[N]] [:relation …]
//! ```
//!
//! - First positional is a dotted model name (`iam.users`, `log.groups`,
//!   `ecs.clusters`, …). Singular renders the first record; plural a list.
//! - `field=value` filters; `[N]` narrows; `:relation` traverses
//!   (`log.group <name> :streams`); `=col1,col2` overrides columns;
//!   `--format=table|json|ndjson|cbor-diag` switches output.
//!
//! The child `aws` process is run with a cleared environment plus only the
//! `AWS_*` vars present in this process's environment (declared explicitly
//! below) and `PATH`/`HOME`.
//!
//! Examples:
//!   aws-cli log.groups
//!   aws-cli iam.users
//!   aws-cli log.group <name> :streams         # traverse a HasMany relation
//!   aws-cli lambda.function <name> :versions
//!   aws-cli --format=json s3.bucket <name> :objects

use anyhow::{Context, Result};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_cli_util::output::{self, OutputFormat};
use vantage_cli_util::vista_cli::{self, AggregateOp, Mode, ModelFactory, Renderer};
use vantage_cli_util::{render_records, render_records_columns};
use vantage_cmd::Cmd;
use vantage_cmd::models::{CmdModelFactory, FactoryMode};
use vantage_types::Record;
use vantage_vista::{ReferenceKind, Vista};

/// AWS env vars forwarded (when present) into the locked child process.
const AWS_ENV_VARS: &[&str] = &[
    "AWS_REGION",
    "AWS_DEFAULT_REGION",
    "AWS_PROFILE",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_CONFIG_FILE",
    "AWS_SHARED_CREDENTIALS_FILE",
];

/// Resolves dotted model names to YAML-backed vistas. Pure delegation to
/// [`CmdModelFactory`] — the only thing the example owns is the
/// `FactoryMode` → `Mode` mapping.
struct CmdFactory(CmdModelFactory);

impl ModelFactory for CmdFactory {
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
}

/// Routes Vista CLI output through the format selected by `--format=…`.
/// `Table` renders a human-friendly comfy-table; the other formats delegate
/// to [`vantage_cli_util::output`] (the format used by portable tests).
struct CmdRenderer {
    format: OutputFormat,
}

impl Renderer for CmdRenderer {
    fn render_list(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    ) {
        match self.format {
            OutputFormat::Table => {
                if let Some(cols) = column_override {
                    render_records_columns(records, cols, &IndexMap::new());
                } else {
                    render_records(records, vista.get_id_column());
                }
                let n = records.len();
                println!("({n} record{})", if n == 1 { "" } else { "s" });
            }
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

fn render_record_table(vista: &Vista, id: &str, record: &Record<CborValue>) {
    let id_field = vista.get_id_column().unwrap_or("id");
    println!("{}: {}", id_field, id);
    let title_fields: Vec<&str> = vista.get_title_columns();
    for tf in &title_fields {
        if *tf == id_field {
            continue;
        }
        if let Some(v) = record.get(*tf) {
            println!("{}: {}", tf, scalar(v));
        }
    }
    println!("--------");
    for (k, v) in record.iter() {
        if k == id_field || title_fields.iter().any(|t| t == k) {
            continue;
        }
        println!("{}: {}", k, scalar(v));
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
        "usage: aws-cli [--region <r>] [--format=<f>] <model> [field=value ...] [[N]] [:relation ...] [@op[:field]]"
    );
    eprintln!("\nFormats: table (default), json, ndjson, cbor-diag");
    eprintln!("\nKnown models:");
    for name in CmdModelFactory::known_names() {
        eprintln!("  {name}");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut region: Option<String> = None;
    let mut format = OutputFormat::Table;
    let mut positional: Vec<String> = Vec::with_capacity(raw.len());

    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        if let Some(value) = arg.strip_prefix("--format=") {
            format =
                OutputFormat::parse(value).with_context(|| format!("unknown --format `{value}`"))?;
        } else if arg == "--region" {
            region = it.next();
        } else if let Some(value) = arg.strip_prefix("--region=") {
            region = Some(value.to_string());
        } else {
            positional.push(arg);
        }
    }

    if positional.is_empty() {
        print_usage();
        std::process::exit(2);
    }

    // Build the locked `aws` datasource, declaring AWS_* env vars present in
    // this process. `--region` overrides AWS_REGION.
    let mut cmd = Cmd::new("aws");
    for var in AWS_ENV_VARS {
        if let Ok(value) = std::env::var(var) {
            cmd = cmd.with_env(*var, value);
        }
    }
    if let Some(r) = region {
        cmd = cmd.with_env("AWS_REGION", r);
    }

    let factory = CmdFactory(CmdModelFactory::new(cmd));
    let renderer = CmdRenderer { format };
    vista_cli::run(&factory, &renderer, &positional).await?;
    Ok(())
}
