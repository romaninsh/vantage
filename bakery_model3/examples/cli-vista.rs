//! Vista-driven multi-source CLI for browsing bakery data.
//!
//! Usage: db-vista <source> <model> [field=value] [[N]] [:relation] [=col1,col2]
//!
//! Sources: csv, sqlite, postgres, mongo, surreal
//! Models : bakery, client, product, order (singular = single record, plural = list)
//!
//! Token grammar matches [`vantage_cli_util::vista_cli`]:
//!   - `<model>`         pick a model (plural = list view, singular = single)
//!   - `id=<value>`      narrow to one record
//!   - `<field>=<value>` add an eq-filter
//!   - `[N]`             pick the N-th row from a list (forces single mode)
//!   - `:<relation>`     traverse a relation using the current row
//!   - `=col1,col2`      override displayed columns
//!
//! Examples:
//!   db-vista csv bakery
//!   db-vista sqlite client id=marty
//!   db-vista sqlite bakery[0] :clients                   # forward (HasMany)
//!   db-vista sqlite client id=marty :bakery              # reverse (HasOne)
//!   db-vista sqlite bakery[0] :clients[0] :bakery        # round-trip
//!   db-vista sqlite bakery[0] :clients[0] :orders        # double-hop

use bakery_model3::*;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use surreal_client::SurrealConnection;
use vantage_cli_util::vista_cli::{self, Mode, ModelFactory, Renderer};
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return Ok(());
    }
    let source = args[0].clone();
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() {
        print_usage();
        return Ok(());
    }

    let renderer = TableRenderer;
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
    println!("Usage: db-vista <source> <model> [field=value] [[N]] [:relation] [=col1,col2]");
    println!();
    println!("Sources: csv, sqlite, postgres, mongo, surreal");
    println!("Models : bakery, client, product, order");
    println!();
    println!("Examples:");
    println!("  db-vista csv bakery");
    println!("  db-vista sqlite client id=marty");
    println!("  db-vista sqlite bakery[0] :clients");
    println!("  db-vista sqlite client id=marty :bakery");
    println!("  db-vista sqlite bakery[0] :clients[0] :bakery       # round-trip");
    println!("  db-vista sqlite bakery[0] :clients[0] :orders       # double-hop");
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

struct TableRenderer;

impl Renderer for TableRenderer {
    fn render_list(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    ) {
        if let Some(cols) = column_override {
            render_records_columns(records, cols, &vista.source_column_types());
        } else {
            render_records(records, vista.get_id_column());
        }
        let n = records.len();
        println!("({n} record{})", if n == 1 { "" } else { "s" });
    }

    fn render_record(
        &self,
        vista: &Vista,
        id: &str,
        record: &Record<CborValue>,
        _relations: &[String],
    ) {
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
        // `list_references` combines foreign resolvers, YAML metadata, and
        // shell-forwarded typed refs — the canonical view for a CLI menu.
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
