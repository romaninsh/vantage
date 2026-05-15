//! `jsonplaceholder_yaml` — same demo as `jsonplaceholder`, but every
//! model is defined in a YAML spec rather than in Rust. Three files
//! under `data/jsonplaceholder/` describe Users, Albums, and Photos;
//! the factory accumulates them via `register_yaml` and the internal
//! registry serves as the model resolver for `:relation` traversal.
//!
//! Usage:
//!
//! ```sh
//! cargo run --example jsonplaceholder_yaml -- users
//! cargo run --example jsonplaceholder_yaml -- users id=1 :albums
//! cargo run --example jsonplaceholder_yaml -- users id=1 ':albums[0]' :photos
//! cargo run --example jsonplaceholder_yaml -- albums id=11 :user
//! cargo run --example jsonplaceholder_yaml -- photos id=42 ':album[0]' :user
//! ```
//!
//! Output matches `jsonplaceholder` line-for-line; only the URLs in
//! transit differ (the YAML declares `endpoint_template` on the
//! `:albums` and `:photos` references, so they take the path-based
//! `/users/{userId}/albums` and `/albums/{albumId}/photos` forms).

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_api_client::{ResponseShape, RestApi, RestApiVistaFactory};
use vantage_cli_util::vista_cli::{self, Mode, ModelFactory, Renderer};
use vantage_types::Record;
use vantage_vista::Vista;

const BASE_URL: &str = "https://jsonplaceholder.typicode.com";

const USERS_YAML: &str = include_str!("../data/jsonplaceholder/users.yaml");
const ALBUMS_YAML: &str = include_str!("../data/jsonplaceholder/albums.yaml");
const PHOTOS_YAML: &str = include_str!("../data/jsonplaceholder/photos.yaml");

// ── Factory + renderer for the vista_cli runner ─────────────────────────

struct JsonPlaceholderFactory {
    inner: Arc<RestApiVistaFactory>,
}

impl JsonPlaceholderFactory {
    fn build(api: RestApi) -> anyhow::Result<Self> {
        let mut factory = RestApiVistaFactory::new(api);
        factory.register_yaml(USERS_YAML)?;
        factory.register_yaml(ALBUMS_YAML)?;
        factory.register_yaml(PHOTOS_YAML)?;
        Ok(Self {
            inner: Arc::new(factory),
        })
    }
}

impl ModelFactory for JsonPlaceholderFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let (canonical, mode) = match name {
            "user" => ("users", Mode::Single),
            "users" => ("users", Mode::List),
            "album" => ("albums", Mode::Single),
            "albums" => ("albums", Mode::List),
            "photo" => ("photos", Mode::Single),
            "photos" => ("photos", Mode::List),
            _ => return None,
        };
        self.inner.build(canonical).ok().map(|v| (v, mode))
    }
}

const KNOWN_MODELS: &[&str] = &["user", "users", "album", "albums", "photo", "photos"];

struct CborRenderer;

impl Renderer for CborRenderer {
    fn render_list(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    ) {
        let id_field = vista.get_id_column().unwrap_or("id").to_string();
        let title_fields: Vec<String> = vista
            .get_title_columns()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

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
            vista
                .get_column_names()
                .into_iter()
                .filter(|c| *c != id_field)
                .take(3)
                .map(|s| s.to_string())
                .collect()
        };

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
        vista: &Vista,
        id: &str,
        record: &Record<CborValue>,
        relations: &[String],
    ) {
        let id_field = vista.get_id_column().unwrap_or("id");
        println!("{}: {}", id_field, id);
        let title_fields: Vec<&str> = vista.get_title_columns();
        for tf in &title_fields {
            if *tf == id_field {
                continue;
            }
            if let Some(v) = record.get(*tf) {
                println!("{}: {}", tf, cbor_short(v));
            }
        }
        println!("--------");
        for (k, v) in record.iter() {
            if k == id_field || title_fields.iter().any(|t| t == k) {
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
        other => format!("{other:?}"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: jsonplaceholder_yaml <model> [field=value ...] [[N]] [:relation ...]");
        eprintln!("\nKnown models:");
        for n in KNOWN_MODELS {
            eprintln!("  {n}");
        }
        std::process::exit(2);
    }

    let api = RestApi::builder(BASE_URL)
        .response_shape(ResponseShape::BareArray)
        .build();
    let factory = JsonPlaceholderFactory::build(api)?;
    let renderer = CborRenderer;
    vista_cli::run(&factory, &renderer, &args)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
