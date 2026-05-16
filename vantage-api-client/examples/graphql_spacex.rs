//! `graphql_spacex` — YAML-driven CLI over the public SpaceX GraphQL API.
//!
//! Ten entities are exposed via hand-curated schema YAML in
//! `examples/schema/`: `launches`, `rockets`, `capsules`, `cores`,
//! `ships`, `payloads`, `missions`, `dragons`, `landpads`,
//! `launchpads`. Each file follows the [`GraphqlApiVistaSpec`] shape
//! — the same one that `GraphqlApiVistaFactory::build_from_spec`
//! lowers into a `Vista`.
//!
//! Several entities declare multiple `title` columns (rockets shows
//! name + type; ships shows name + type; capsules and dragons too) —
//! `vista_cli` surfaces all of them in the list view so identifying
//! rows doesn't depend on remembering which column is the label.
//!
//! SDL reference:
//! <https://studio.apollographql.com/public/SpaceX-pxxbxen/variant/current/schema/sdl>
//!
//! Usage:
//!
//! ```sh
//! cargo run --example graphql_spacex -- launches
//! cargo run --example graphql_spacex -- launches mission_name=FalconSat
//! cargo run --example graphql_spacex -- rockets
//! cargo run --example graphql_spacex -- capsules status=active
//! cargo run --example graphql_spacex -- ships
//! cargo run --example graphql_spacex -- cores
//! ```
//!
//! **Relations note**: the SpaceX schema exposes cross-entity links as
//! server-resolved *nested* objects (`launch.rocket`, `launch.ships`,
//! `capsule.missions`), not as flat foreign-key columns. The adapter's
//! `with_many`/`with_one` mechanism is built for flat FKs, so the
//! handful of nominally relational fields here (`launches.mission_id`
//! is a *list*, `capsules.find.mission` matches against a nested
//! `CapsuleMission.name` rather than the top-level `missions` table)
//! don't compose into clean traversals. Nested-selection adapter mode
//! is the right tool — tracked for a future phase.
//!
//! The endpoint defaults to a community mirror and can be overridden
//! via `SPACEX_ENDPOINT`. Community mirrors come and go — if a 4xx/5xx
//! lands, point the env var at a working endpoint.

use ciborium::Value as CborValue;
use indexmap::IndexMap;

use vantage_api_client::{GraphqlApi, GraphqlApiVistaSpec};
use vantage_cli_util::vista_cli::{self, Mode, ModelFactory, Renderer};
use vantage_types::Record;
use vantage_vista::{Vista, VistaFactory};

/// Default SpaceX GraphQL endpoint. Override with `SPACEX_ENDPOINT`.
const DEFAULT_ENDPOINT: &str = "https://spacex-api.fly.dev/graphql";

/// Schema files distributed alongside this example. Each entry is a
/// (root-name, YAML) pair; the YAML is `include_str!`'d so the binary
/// is self-contained.
const SCHEMA_FILES: &[(&str, &str)] = &[
    ("launches", include_str!("schema/launches.yaml")),
    ("rockets", include_str!("schema/rockets.yaml")),
    ("capsules", include_str!("schema/capsules.yaml")),
    ("cores", include_str!("schema/cores.yaml")),
    ("ships", include_str!("schema/ships.yaml")),
    ("payloads", include_str!("schema/payloads.yaml")),
    ("missions", include_str!("schema/missions.yaml")),
    ("dragons", include_str!("schema/dragons.yaml")),
    ("landpads", include_str!("schema/landpads.yaml")),
    ("launchpads", include_str!("schema/launchpads.yaml")),
];

// ── Factory ──────────────────────────────────────────────────────────────
//
// `SpaceXFactory` loads each entity's YAML spec at startup and feeds it
// through `GraphqlApiVistaFactory::build_from_spec`. The resulting Vistas
// share a single `GraphqlApi` clone (one HTTP client, one endpoint).

struct SpaceXFactory {
    api: GraphqlApi,
    specs: IndexMap<String, GraphqlApiVistaSpec>,
}

impl SpaceXFactory {
    fn new(api: GraphqlApi) -> anyhow::Result<Self> {
        let mut specs = IndexMap::new();
        for (name, yaml) in SCHEMA_FILES {
            let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml)
                .map_err(|e| anyhow::anyhow!("parse {}.yaml: {}", name, e))?;
            specs.insert((*name).to_string(), spec);
        }
        Ok(Self { api, specs })
    }

    fn vista_for(&self, name: &str) -> Option<Vista> {
        let spec = self.specs.get(name)?.clone();
        self.api.vista_factory().build_from_spec(spec).ok()
    }
}

impl ModelFactory for SpaceXFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        // SpaceX root fields are all plural lists; we treat both
        // singular ("launch") and plural ("launches") as the same list
        // — the vista_cli runner handles the field=value narrowing.
        let key = singular_to_plural(name)?;
        let mode = if name.ends_with('s') { Mode::List } else { Mode::Single };
        self.vista_for(key).map(|v| (v, mode))
    }
}

/// Map both the singular and plural CLI forms onto the schema key.
/// Returns `None` for anything we don't have a schema file for.
fn singular_to_plural(name: &str) -> Option<&'static str> {
    Some(match name {
        "launch" | "launches" => "launches",
        "rocket" | "rockets" => "rockets",
        "capsule" | "capsules" => "capsules",
        "core" | "cores" => "cores",
        "ship" | "ships" => "ships",
        "payload" | "payloads" => "payloads",
        "mission" | "missions" => "missions",
        "dragon" | "dragons" => "dragons",
        "landpad" | "landpads" => "landpads",
        "launchpad" | "launchpads" => "launchpads",
        _ => return None,
    })
}

const KNOWN_MODELS: &[&str] = &[
    "launch", "launches",
    "rocket", "rockets",
    "capsule", "capsules",
    "core", "cores",
    "ship", "ships",
    "payload", "payloads",
    "mission", "missions",
    "dragon", "dragons",
    "landpad", "landpads",
    "launchpad", "launchpads",
];

// ── Renderer ─────────────────────────────────────────────────────────────
//
// Identical to the `jsonplaceholder` example's renderer — title field
// from metadata, three-column fallback otherwise, CBOR-aware scalar
// stringification.

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
                .map(|c| if c == "id" { id_field.clone() } else { c.clone() })
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
        eprintln!("usage: graphql_spacex <model> [field=value ...] [[N]]");
        eprintln!("\nKnown models:");
        for n in KNOWN_MODELS {
            eprintln!("  {n}");
        }
        eprintln!("\nEndpoint (override via $SPACEX_ENDPOINT):");
        eprintln!("  {}", endpoint());
        eprintln!("\nSchemas:");
        for (n, _) in SCHEMA_FILES {
            eprintln!("  examples/schema/{n}.yaml");
        }
        std::process::exit(2);
    }

    let api = GraphqlApi::new(endpoint());
    let factory = SpaceXFactory::new(api)?;
    let renderer = CborRenderer;
    vista_cli::run(&factory, &renderer, &args)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

fn endpoint() -> String {
    std::env::var("SPACEX_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string())
}
