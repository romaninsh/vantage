//! `k8s-cli` — Vista-driven CLI over `vantage-kubernetes`.
//!
//! Argument grammar matches [`vantage_cli_util::vista_cli`]. Common shapes:
//!
//! ```text
//! k8s-cli [--format=<f>] <model> [token …]
//!
//! k8s-cli core.pods
//! k8s-cli core.pods namespace=demo
//! k8s-cli apps.deployment name=web :pods
//! k8s-cli core.nodes =name,cpuCapacityMillicores,memCapacityBytes
//! k8s-cli --format=json metrics.node_metrics
//! ```
//!
//! - First positional is a dotted model name (`core.pods`,
//!   `apps.deployments`, `metrics.node_metrics`, …). Singular names render
//!   the first record; plurals render a list.
//! - `field=value` filters; `[N]` / `[+col]` slice & sort; `:relation`
//!   traverses a `with_many`; `=col1,col2` overrides columns;
//!   `@count` / `@sum:field` aggregate.
//! - `--format=table|json|ndjson|cbor-diag` switches output.
//!
//! Connects via the current kubeconfig context (honours `$KUBECONFIG`).

use anyhow::{Context, Result};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_cli_util::output::{self, OutputFormat};
use vantage_cli_util::vista_cli::{self, AggregateOp, Mode, ModelFactory, Renderer};
use vantage_cli_util::{render_records_columns, render_records_typed};
use vantage_kubernetes::KubernetesCluster;
use vantage_kubernetes::models::{Factory, FactoryMode};
use vantage_types::{Record, RichText, Style, TerminalRender};
use vantage_vista::{ReferenceKind, Vista};

/// Adapts [`Factory`] to `vantage-cli-util`'s vista_cli factory trait.
struct KubeFactoryAdapter(Factory);

impl ModelFactory for KubeFactoryAdapter {
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

/// A `CborValue` that knows how to render itself as a terminal cell.
struct KubeValue(CborValue);

impl TerminalRender for KubeValue {
    fn render(&self) -> RichText {
        match &self.0 {
            CborValue::Text(s) => RichText::plain(s.clone()),
            CborValue::Integer(i) => RichText::plain(i128::from(*i).to_string()),
            CborValue::Float(f) => RichText::plain(f.to_string()),
            CborValue::Bool(b) => b.render(),
            CborValue::Null => RichText::styled("—", Style::Muted),
            CborValue::Bytes(b) => RichText::plain(format!("<{} bytes>", b.len())),
            other => RichText::plain(format!("{other:?}")),
        }
    }
}

struct KubeRenderer {
    format: OutputFormat,
}

impl Renderer for KubeRenderer {
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
            OutputFormat::Table => println!("{label} = {}", KubeValue(value.clone()).render()),
            _ => print!("{}", output::render_scalar(self.format, &label, value)),
        }
    }
}

/// Vista column metadata as `IndexMap<name, &'static str>` for the table
/// helpers (which want `&'static str` type names). We leak each string
/// once — the CLI exits shortly after rendering.
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

fn wrap_records(
    records: &IndexMap<String, Record<CborValue>>,
) -> IndexMap<String, Record<KubeValue>> {
    records
        .iter()
        .map(|(id, record)| {
            let wrapped: Record<KubeValue> = record
                .iter()
                .map(|(k, v)| (k.clone(), KubeValue(v.clone())))
                .collect();
            (id.clone(), wrapped)
        })
        .collect()
}

fn render_list_table(
    vista: &Vista,
    records: &IndexMap<String, Record<CborValue>>,
    column_override: Option<&[String]>,
) {
    let id_field = vista.get_id_column().map(str::to_string);
    let column_types = column_types_static(vista);
    let wrapped = wrap_records(records);

    if let Some(cols) = column_override {
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
        render_records_columns(&wrapped, &resolved, &column_types);
        return;
    }

    // Default: id + every declared column (Lens-style wide listing).
    render_records_typed(&wrapped, id_field.as_deref(), &column_types);
}

fn render_record_table(vista: &Vista, id: &str, record: &Record<CborValue>) {
    let id_field = vista.get_id_column().map(str::to_string);
    let title_fields: Vec<String> = vista
        .get_title_columns()
        .into_iter()
        .map(str::to_string)
        .collect();

    if let Some(ref name) = id_field {
        println!("{name}: {id}");
    } else {
        println!("id: {id}");
    }
    for tf in &title_fields {
        if Some(tf.as_str()) == id_field.as_deref() {
            continue;
        }
        if let Some(v) = record.get(tf) {
            println!("{tf}: {}", KubeValue(v.clone()).render());
        }
    }

    println!("--------");

    for name in vista.get_column_names() {
        if Some(name) == id_field.as_deref() || title_fields.iter().any(|t| t == name) {
            continue;
        }
        if let Some(v) = record.get(name) {
            println!("{name}: {}", KubeValue(v.clone()).render());
        }
    }

    let refs = vista.list_references();
    if !refs.is_empty() {
        println!("\nRelations:");
        for (name, kind) in refs {
            let marker = match kind {
                ReferenceKind::HasOne => "→ one",
                ReferenceKind::HasMany => "↠ many",
            };
            println!("  :{name}  {marker}");
        }
    }
}

fn print_usage() {
    eprintln!("usage: k8s-cli [--format=<f>] <model> [field=value ...] [[N]] [:relation ...] [@op[:field]]");
    eprintln!("\nFormats: table (default), json, ndjson, cbor-diag");
    eprintln!("\nKnown models:");
    for name in Factory::known_names() {
        eprintln!("  {name}");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut format = OutputFormat::Table;
    let mut positional: Vec<String> = Vec::with_capacity(raw.len());

    for arg in raw {
        if let Some(value) = arg.strip_prefix("--format=") {
            format = OutputFormat::parse(value).with_context(|| format!("unknown --format `{value}`"))?;
        } else {
            positional.push(arg);
        }
    }

    if positional.is_empty() {
        print_usage();
        std::process::exit(2);
    }

    let cluster = KubernetesCluster::from_default()
        .await
        .context("could not connect to a cluster — is your kubeconfig context set? (try `kubectl config current-context`)")?;

    let factory = KubeFactoryAdapter(Factory::new(cluster));
    let renderer = KubeRenderer { format };
    vista_cli::run(&factory, &renderer, &positional).await?;
    Ok(())
}
