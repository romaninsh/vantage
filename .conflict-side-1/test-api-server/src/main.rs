use anyhow::Result;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashSet,
    sync::Arc,
};
use tokio::{select, signal, sync::RwLock};
use uuid::Uuid;
use warp::{http::StatusCode, reply::Response, Filter, Reply};

fn load_cities(path: &str) -> IndexMap<String, Vec<(String, i64)>> {
    let mut reader = csv::ReaderBuilder::new().from_path(path).unwrap();
    let mut countries: IndexMap<String, Vec<(String, i64)>> = IndexMap::new();

    for result in reader.records() {
        let record = result.unwrap();
        let city_name = record.get(1).unwrap_or_default().to_string();
        let country_name = record.get(7).unwrap_or_default().trim_matches('"').to_string();
        let population: i64 = record.get(14).and_then(|s| s.parse().ok()).unwrap_or(0);

        countries
            .entry(country_name)
            .or_default()
            .push((city_name, population));
    }

    for cities in countries.values_mut() {
        cities.sort_by(|a, b| b.1.cmp(&a.1));
    }
    countries.sort_keys();
    countries
}

struct AppState {
    tokens: RwLock<HashSet<String>>,
    countries: IndexMap<String, Vec<(String, i64)>>,
}

async fn auth(state: Arc<AppState>) -> Result<impl warp::Reply, warp::Rejection> {
    let token = Uuid::new_v4().to_string();
    state.tokens.write().await.insert(token.clone());
    Ok(warp::reply::json(&json!({"token": token})))
}

#[derive(Deserialize)]
struct PagedQuery {
    page: Option<usize>,
    per_page: Option<usize>,
}

fn pager<T, I>(items: I, query: PagedQuery, fx: impl Fn(T) -> serde_json::Value) -> Response
where
    T: serde::Serialize,
    I: IntoIterator<Item = T>,
    I::IntoIter: ExactSizeIterator,
{
    let page = query.page.unwrap_or(1);
    let page_size = query.per_page.unwrap_or(10).min(10);
    let skip = (page - 1) * page_size;

    let iter = items.into_iter();
    let total_items = iter.len();
    let total_pages = (total_items + page_size - 1) / page_size;

    let page_items: Vec<serde_json::Value> = iter.skip(skip).take(page_size).map(fx).collect();

    let has_next = !page_items.is_empty() && page < total_pages;
    let has_prev = page > 1;

    warp::reply::with_status(
        warp::reply::json(&json!({
            "data": page_items,
            "pagination": {
                "page": page,
                "per_page": page_size,
                "total": total_items,
                "total_pages": total_pages,
                "has_next": has_next,
                "has_prev": has_prev
            }
        })),
        StatusCode::OK,
    )
    .into_response()
}

async fn countries_handler(
    _token: String,
    state: Arc<AppState>,
    query: PagedQuery,
) -> Result<Response, warp::Rejection> {
    Ok(pager(
        state.countries.keys().collect::<Vec<&String>>(),
        query,
        |country: &String| json!({"name": country}),
    ))
}

async fn cities_handler(
    country: String,
    _token: String,
    state: Arc<AppState>,
    query: PagedQuery,
) -> Result<Response, warp::Rejection> {
    let decoded_country = urlencoding::decode(&country).unwrap_or_default();

    let country_cities = match state.countries.get(decoded_country.as_ref()) {
        Some(cities) => cities,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({"error": "Country not found"})),
                StatusCode::NOT_FOUND,
            )
            .into_response());
        }
    };

    Ok(pager(
        country_cities,
        query,
        |x| json!({"name": x.0, "population": x.1}),
    ))
}

#[derive(Debug)]
struct Unauthorized;
impl warp::reject::Reject for Unauthorized {}

fn with_auth(
    state: Arc<AppState>,
) -> impl Filter<Extract = (), Error = warp::Rejection> + Clone {
    warp::any()
        .map(move || state.clone())
        .and(warp::header("Authorization"))
        .and_then(|state: Arc<AppState>, auth: String| async move {
            if !auth.starts_with("Bearer ") {
                return Err(warp::reject::custom(Unauthorized));
            }
            let token = &auth[7..];
            match state.tokens.read().await.get(token) {
                None => Err(warp::reject::custom(Unauthorized)),
                Some(_) => Ok(()),
            }
        })
        .untuple_one()
}

fn with_state(
    state: Arc<AppState>,
) -> impl Filter<Extract = (Arc<AppState>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

fn routes(
    state: Arc<AppState>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let health = warp::path!("health").map(|| warp::reply::html("OK"));

    let auth_route = warp::path!("auth")
        .and(warp::post())
        .and(with_state(state.clone()))
        .and_then(auth);

    let cities = warp::path!("countries" / String / "cities")
        .and(warp::get())
        .and(with_state(state.clone()))
        .and(with_auth(state.clone()))
        .and(warp::query::<PagedQuery>())
        .and_then(|country, state, query| cities_handler(country, String::new(), state, query));

    let countries = warp::path("countries")
        .and(warp::get())
        .and(with_state(state.clone()))
        .and(with_auth(state.clone()))
        .and(warp::query::<PagedQuery>())
        .and_then(|state, query| countries_handler(String::new(), state, query));

    health.or(auth_route).or(cities).or(countries)
}

async fn handle_rejection(
    err: warp::Rejection,
) -> Result<impl warp::Reply, std::convert::Infallible> {
    if err.find::<Unauthorized>().is_some() {
        Ok(warp::reply::with_status(
            warp::reply::json(&json!({"error": "Unauthorized"})),
            StatusCode::UNAUTHORIZED,
        )
        .into_response())
    } else if err.find::<warp::reject::MethodNotAllowed>().is_some() {
        Ok(warp::reply::with_status(
            warp::reply::json(&json!({"error": "Method not allowed"})),
            StatusCode::METHOD_NOT_ALLOWED,
        )
        .into_response())
    } else {
        Ok(warp::reply::with_status(
            warp::reply::json(&json!({"error": "Internal server error"})),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let csv_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/cities.csv".to_string());

    println!("Loading cities from {}...", csv_path);
    let countries = load_cities(&csv_path);
    let total_cities: usize = countries.values().map(|c| c.len()).sum();
    println!(
        "Loaded {} countries with {} cities",
        countries.len(),
        total_cities
    );

    let state = Arc::new(AppState {
        tokens: RwLock::new(HashSet::new()),
        countries,
    });

    let routes = routes(state).recover(handle_rejection);

    println!("Starting server on http://127.0.0.1:3030");

    let work = warp::serve(routes).run(([127, 0, 0, 1], 3030));
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
        println!("\nShutting down...");
    };

    select! {
        _ = work => {},
        _ = ctrl_c => {},
    }

    Ok(())
}
