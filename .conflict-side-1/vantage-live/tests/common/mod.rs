use vantage_dataset::im::{ImDataSource, ImTable};

// Re-export bakery_model3 entities and connection
pub use bakery_model3::{connect_surrealdb, surrealdb, Bakery, Client, Product};

// Shared ImDataSource for all cache tables in tests
static IM_DATASOURCE: std::sync::OnceLock<ImDataSource> = std::sync::OnceLock::new();

pub fn im_datasource() -> ImDataSource {
    IM_DATASOURCE.get_or_init(|| ImDataSource::new()).clone()
}

pub fn setup_cache<E>() -> ImTable<E>
where
    E: serde::Serialize + serde::de::DeserializeOwned,
{
    let ds = im_datasource();
    // Use unique table name per cache to avoid conflicts
    ImTable::new(&ds, &format!("cache_{}", uuid::Uuid::new_v4()))
}

// Known IDs from v2.surql for testing
pub const BAKERY_HILL_VALLEY: &str = "hill_valley";
pub const CLIENT_MARTY: &str = "marty";
pub const CLIENT_DOC: &str = "doc";
pub const CLIENT_BIFF: &str = "biff";
pub const PRODUCT_FLUX_CUPCAKE: &str = "flux_cupcake";
pub const PRODUCT_DELOREAN_DONUT: &str = "delorean_donut";
pub const PRODUCT_TIME_TART: &str = "time_tart";
