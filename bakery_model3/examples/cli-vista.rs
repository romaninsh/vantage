//! Vista-driven multi-source CLI for browsing bakery data.
//!
//! Usage:
//!   db-vista [--format=<fmt>] <source> <model> [token …]
//!
//! Sources: csv, sqlite, postgres, mongo, surreal
//! Models : bakery, client, product, order (singular = single, plural = list)
//! Formats: table (default), json, ndjson, cbor-diag
//!
//! Token grammar matches [`vantage_cli_util::vista_cli`]; see that
//! crate's docs for the full vocabulary (operators, sort+slice
//! brackets, search, aggregates, locators, JSON-typed values).
//!
//! Examples:
//!   db-vista csv bakery
//!   db-vista sqlite client id=marty
//!   db-vista sqlite bakery[0] :clients                   # forward (HasMany)
//!   db-vista sqlite client id=marty :bakery              # reverse (HasOne)
//!   db-vista --format=cbor-diag csv clients              # lossless for golden tests
//!   db-vista --format=json csv 'clients[+name:0]'        # sort then narrow

use bakery_model3::*;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use surreal_client::SurrealConnection;
use vantage_cli_util::output::{self, OutputFormat};
use vantage_cli_util::vista_cli::{self, AggregateOp, Mode, ModelFactory, Renderer};
use vantage_cli_util::{render_records, render_records_columns};
use vantage_csv::Csv;
use vantage_types::Record;
use vantage_vista::Vista;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> vantage_core::Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() {
        print_usage();
        return Ok(());
    }

    // Strip out any `--format=<fmt>` flag (position-agnostic). Everything
    // else is treated as a positional and forwarded.
    let mut format = OutputFormat::Table;
    let mut positional: Vec<String> = Vec::with_capacity(raw.len());
    for arg in raw {
        if let Some(value) = arg.strip_prefix("--format=") {
            format = OutputFormat::parse(value).ok_or_else(|| {
                vantage_core::error!(format!(
                    "Unknown --format `{value}` — pick table, json, ndjson, or cbor-diag"
                ))
            })?;
        } else {
            positional.push(arg);
        }
    }

    if positional.is_empty() {
        print_usage();
        return Ok(());
    }
    let source = positional[0].clone();
    let rest: Vec<String> = positional.into_iter().skip(1).collect();

    if rest.is_empty() {
        print_usage();
        return Ok(());
    }

    let renderer = MultiFormatRenderer { format };
    match source.as_str() {
        "csv" => {
            let factory = CsvFactory {
                csv: Csv::new("bakery_model3/data"),
            };
            vista_cli::run(&factory, &renderer, &rest).await
        }
        "sqlite" => {
            let db = SqliteDB::connect("sqlite:target/bakery.sqlite")
                .await
                .map_err(|e| {
                    vantage_core::error!("Failed to connect to SQLite", details = e.to_string())
                })?;
            vista_cli::run(&SqliteFactory { db }, &renderer, &rest).await
        }
        "postgres" => {
            let url = std::env::var("POSTGRES_URL").unwrap_or_else(|_| {
                "postgres://vantage:vantage@localhost:5433/vantage".to_string()
            });
            let db = PostgresDB::connect(&url).await.map_err(|e| {
                vantage_core::error!("Failed to connect to PostgreSQL", details = e.to_string())
            })?;
            vista_cli::run(&PostgresFactory { db }, &renderer, &rest).await
        }
        "mongo" => {
            let url = std::env::var("MONGODB_URL")
                .unwrap_or_else(|_| "mongodb://localhost:27017".to_string());
            let db_name = std::env::var("MONGODB_DB").unwrap_or_else(|_| "vantage".to_string());
            let db = MongoDB::connect(&url, &db_name).await.map_err(|e| {
                vantage_core::error!("Failed to connect to MongoDB", details = e.to_string())
            })?;
            vista_cli::run(&MongoFactory { db }, &renderer, &rest).await
        }
        "surreal" => {
            let dsn = std::env::var("SURREALDB_URL")
                .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
            let client = SurrealConnection::dsn(&dsn)
                .map_err(|e| {
                    vantage_core::error!("Invalid SurrealDB DSN", details = e.to_string())
                })?
                .connect()
                .await
                .map_err(|e| {
                    vantage_core::error!("Failed to connect to SurrealDB", details = e.to_string())
                })?;
            let db = SurrealDB::new(client);
            vista_cli::run(&SurrealFactory { db }, &renderer, &rest).await
        }
        other => Err(vantage_core::error!(format!(
            "Unknown source `{other}` — use csv, sqlite, postgres, mongo, or surreal"
        ))),
    }
}

fn print_usage() {
    println!("Usage: db-vista [--format=<fmt>] <source> <model> [token …]");
    println!();
    println!("Sources: csv, sqlite, postgres, mongo, surreal");
    println!("Models : bakery, client, product, order");
    println!("Formats: table (default), json, ndjson, cbor-diag");
    println!();
    println!("Examples:");
    println!("  db-vista csv bakery");
    println!("  db-vista sqlite client id=marty");
    println!("  db-vista sqlite bakery[0] :clients");
    println!("  db-vista --format=cbor-diag csv clients");
    println!("  db-vista --format=json csv 'clients[+name:0]'");
}

// ── Per-source factories ─────────────────────────────────────────────────

fn mode_for(name: &str) -> Option<Mode> {
    match name {
        // Singular = pick first record; plural = list view. Either spelling
        // works so users can type whichever fits the sentence.
        "bakery" | "client" | "product" | "order" => Some(Mode::Single),
        "bakeries" | "clients" | "products" | "orders" => Some(Mode::List),
        _ => None,
    }
}

struct CsvFactory {
    csv: Csv,
}

impl ModelFactory for CsvFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let mode = mode_for(name)?;
        let csv = self.csv.clone();
        let factory = csv.vista_factory();
        let vista = match name {
            "bakery" | "bakeries" => factory.from_table(Bakery::csv_table(csv)).ok()?,
            "client" | "clients" => factory.from_table(Client::csv_table(csv)).ok()?,
            "product" | "products" => factory.from_table(Product::csv_table(csv)).ok()?,
            "order" | "orders" => factory.from_table(Order::csv_table(csv)).ok()?,
            _ => return None,
        };
        Some((vista, mode))
    }
}

struct SqliteFactory {
    db: SqliteDB,
}

impl ModelFactory for SqliteFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let mode = mode_for(name)?;
        let db = self.db.clone();
        let factory = db.vista_factory();
        let vista = match name {
            "bakery" | "bakeries" => factory.from_table(Bakery::sqlite_table(db)).ok()?,
            "client" | "clients" => factory.from_table(Client::sqlite_table(db)).ok()?,
            "product" | "products" => factory.from_table(Product::sqlite_table(db)).ok()?,
            "order" | "orders" => factory.from_table(Order::sqlite_table(db)).ok()?,
            _ => return None,
        };
        Some((vista, mode))
    }
}

struct PostgresFactory {
    db: PostgresDB,
}

impl ModelFactory for PostgresFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let mode = mode_for(name)?;
        let db = self.db.clone();
        let factory = db.vista_factory();
        let vista = match name {
            "bakery" | "bakeries" => factory.from_table(Bakery::postgres_table(db)).ok()?,
            "client" | "clients" => factory.from_table(Client::postgres_table(db)).ok()?,
            "product" | "products" => factory.from_table(Product::postgres_table(db)).ok()?,
            "order" | "orders" => factory.from_table(Order::postgres_table(db)).ok()?,
            _ => return None,
        };
        Some((vista, mode))
    }
}

struct MongoFactory {
    db: MongoDB,
}

impl ModelFactory for MongoFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let mode = mode_for(name)?;
        let db = self.db.clone();
        let factory = db.vista_factory();
        let vista = match name {
            "bakery" | "bakeries" => factory.from_table(Bakery::mongo_table(db)).ok()?,
            "client" | "clients" => factory.from_table(Client::mongo_table(db)).ok()?,
            "product" | "products" => factory.from_table(Product::mongo_table(db)).ok()?,
            "order" | "orders" => factory.from_table(Order::mongo_table(db)).ok()?,
            _ => return None,
        };
        Some((vista, mode))
    }
}

struct SurrealFactory {
    db: SurrealDB,
}

impl ModelFactory for SurrealFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let mode = mode_for(name)?;
        let db = self.db.clone();
        let factory = db.vista_factory();
        let vista = match name {
            "bakery" | "bakeries" => factory.from_table(Bakery::surreal_table(db)).ok()?,
            "client" | "clients" => factory.from_table(Client::surreal_table(db)).ok()?,
            "product" | "products" => factory.from_table(Product::surreal_table(db)).ok()?,
            "order" | "orders" => factory.from_table(Order::surreal_table(db)).ok()?,
            _ => return None,
        };
        Some((vista, mode))
    }
}

// ── Renderer ─────────────────────────────────────────────────────────────

/// Routes Vista CLI output through the format selected by `--format=…`.
/// `Table` keeps the legacy comfy-table rendering for humans; the other
/// formats delegate to [`vantage_cli_util::output`] which is used by
/// driver-portable tests.
struct MultiFormatRenderer {
    format: OutputFormat,
}

impl Renderer for MultiFormatRenderer {
    fn render_list(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    ) {
        match self.format {
            OutputFormat::Table => {
                if let Some(cols) = column_override {
                    render_records_columns(records, cols, &vista.source_column_types());
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
                vantage_vista::ReferenceKind::HasOne => "→ one",
                vantage_vista::ReferenceKind::HasMany => "↠ many",
            };
            println!("  :{name}  {marker}");
        }
    }
}

trait VistaColumnTypes {
    fn source_column_types(&self) -> IndexMap<String, &'static str>;
}

impl VistaColumnTypes for Vista {
    fn source_column_types(&self) -> IndexMap<String, &'static str> {
        // Column metadata isn't directly exposed for typing; fall back to
        // an empty map so the renderer uses defaults.
        IndexMap::new()
    }
}

fn scalar(v: &CborValue) -> String {
    use ciborium::Value as C;
    match v {
        C::Text(s) => s.clone(),
        C::Integer(i) => i128::from(*i).to_string(),
        C::Float(f) => f.to_string(),
        C::Bool(b) => b.to_string(),
        C::Null => "—".to_string(),
        C::Bytes(b) => format!("<{} bytes>", b.len()),
        other => format!("{other:?}"),
    }
}
