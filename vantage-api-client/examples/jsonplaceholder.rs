//! `jsonplaceholder` — model-driven CLI over the public
//! <https://jsonplaceholder.typicode.com> demo API.
//!
//! Three typed entities live behind it: User, Album, Photo. Albums
//! belong to a User; Photos belong to an Album. The factory wires each
//! parent's `with_many` reference to a child table built around a URI
//! template — for example, traversing `users id=1 :albums` lowers to
//! `GET /users/1/albums`, with `userId` peeled out of the conditions
//! and substituted into the URL path.
//!
//! Usage:
//!
//! ```sh
//! cargo run --example jsonplaceholder -- users
//! cargo run --example jsonplaceholder -- users id=1
//! cargo run --example jsonplaceholder -- users id=1 :albums
//! cargo run --example jsonplaceholder -- users id=1 :albums[0] :photos
//! cargo run --example jsonplaceholder -- albums userId=1
//! ```
//!
//! Token grammar matches `vantage_cli_util::vista_cli`: model name,
//! `field=value` filters, `[N]` index selectors, `:relation`
//! traversals. The first token must be a model name.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use vantage_api_client::{RestApi, ResponseShape};
use vantage_cli_util::vista_cli::{self, Mode, ModelFactory, Renderer};
use vantage_table::table::Table;
use vantage_types::Record;
use vantage_vista::Vista;

const BASE_URL: &str = "https://jsonplaceholder.typicode.com";

// ── Entity types ─────────────────────────────────────────────────────────
//
// jsonplaceholder returns one JSON object per record. Each field is
// either present (with a concrete value) or missing — we model with
// `Option<...>` to keep `TryFromRecord` lenient against future schema
// drift. Field names match the API verbatim (camelCase) so id-based
// joins and URI templating line up without aliasing.

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct User {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Album {
    pub id: Option<i64>,
    #[serde(rename = "userId")]
    pub user_id: Option<i64>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Photo {
    pub id: Option<i64>,
    #[serde(rename = "albumId")]
    pub album_id: Option<i64>,
    pub title: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "thumbnailUrl")]
    pub thumbnail_url: Option<String>,
}

// ── Table factories ──────────────────────────────────────────────────────
//
// Each model exposes two factories:
//   * the top-level table — `users`, `albums`, `photos` — used when the
//     entity is queried in its own right;
//   * a URI-template variant — `users/{userId}/albums`,
//     `albums/{albumId}/photos` — used as a `with_many` build target
//     so traversing from a parent narrows directly to the nested
//     endpoint.
//
// The condition produced by RestApi's `related_in_condition`
// (`userId = <parent_id>`) is consumed by the template's `{userId}`
// substitution at request time; nothing leaks into the query string.

impl User {
    pub fn api_table(api: RestApi) -> Table<RestApi, User> {
        Table::new("users", api)
            .with_id_column("id")
            .with_title_column_of::<Option<String>>("name")
            .with_column_of::<Option<String>>("username")
            .with_column_of::<Option<String>>("email")
            .with_column_of::<Option<String>>("phone")
            .with_column_of::<Option<String>>("website")
            .with_many("albums", "userId", Album::api_table_for_user)
    }
}

impl Album {
    pub fn api_table(api: RestApi) -> Table<RestApi, Album> {
        Self::columns(Table::new("albums", api))
    }

    /// Nested form used when traversing from a User. The `{userId}`
    /// placeholder is resolved by `RestApi`'s template substitution at
    /// request time from the eq-condition wired up by `with_many`.
    pub fn api_table_for_user(api: RestApi) -> Table<RestApi, Album> {
        Self::columns(Table::new("users/{userId}/albums", api))
    }

    fn columns(t: Table<RestApi, Album>) -> Table<RestApi, Album> {
        t.with_id_column("id")
            .with_title_column_of::<Option<String>>("title")
            .with_column_of::<Option<i64>>("userId")
            .with_one("user", "userId", User::api_table)
            .with_many("photos", "albumId", Photo::api_table_for_album)
    }
}

impl Photo {
    pub fn api_table(api: RestApi) -> Table<RestApi, Photo> {
        Self::columns(Table::new("photos", api))
    }

    pub fn api_table_for_album(api: RestApi) -> Table<RestApi, Photo> {
        Self::columns(Table::new("albums/{albumId}/photos", api))
    }

    fn columns(t: Table<RestApi, Photo>) -> Table<RestApi, Photo> {
        t.with_id_column("id")
            .with_title_column_of::<Option<String>>("title")
            .with_column_of::<Option<i64>>("albumId")
            .with_column_of::<Option<String>>("url")
            .with_column_of::<Option<String>>("thumbnailUrl")
            .with_one("album", "albumId", Album::api_table)
    }
}

// ── Factory + renderer for the vista_cli runner ─────────────────────────

struct JsonPlaceholderFactory {
    api: RestApi,
}

impl JsonPlaceholderFactory {
    fn new(api: RestApi) -> Self {
        Self { api }
    }

    fn vista_for(&self, name: &str) -> Option<Vista> {
        let factory = self.api.vista_factory();
        match name {
            "user" | "users" => factory.from_table(User::api_table(self.api.clone())).ok(),
            "album" | "albums" => factory.from_table(Album::api_table(self.api.clone())).ok(),
            "photo" | "photos" => factory.from_table(Photo::api_table(self.api.clone())).ok(),
            _ => None,
        }
    }
}

impl ModelFactory for JsonPlaceholderFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let mode = match name {
            "user" | "album" | "photo" => Mode::Single,
            "users" | "albums" | "photos" => Mode::List,
            _ => return None,
        };
        self.vista_for(name).map(|v| (v, mode))
    }
}

const KNOWN_MODELS: &[&str] = &[
    "user", "users", "album", "albums", "photo", "photos",
];

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
            // Fall back to the first three non-id columns from the
            // declared schema — keeps the table tidy on noisy APIs.
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
        eprintln!(
            "usage: jsonplaceholder <model> [field=value ...] [[N]] [:relation ...]"
        );
        eprintln!("\nKnown models:");
        for n in KNOWN_MODELS {
            eprintln!("  {n}");
        }
        std::process::exit(2);
    }

    // jsonplaceholder returns bare JSON arrays and supports the
    // JSON-Server `_page` / `_limit` conventions out of the box.
    let api = RestApi::builder(BASE_URL)
        .response_shape(ResponseShape::BareArray)
        .build();

    let factory = JsonPlaceholderFactory::new(api);
    let renderer = CborRenderer;
    vista_cli::run(&factory, &renderer, &args)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
