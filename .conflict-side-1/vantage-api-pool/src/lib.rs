// API Client Pool Library
mod stats;

pub use stats::Average;
pub use stats::Stats;

pub mod rate_limit;

pub use rate_limit::KeyedRateLimiter;
pub use rate_limit::RateLimitPolicyEnforcer;
pub use rate_limit::RateLimiter;

mod eventual_request;
pub use eventual_request::EventualRequest;

mod client_pool;
pub use client_pool::HttpClientPool;

mod matcher;
pub use matcher::EventualRequestMatcher;

mod aww_pool;
pub use aww_pool::AwwPool;

mod paginator;
pub use paginator::ItemStream;
pub use paginator::ItemStream4;
pub use paginator::PaginatedStream;
pub use paginator::PaginatedStream2;
pub use paginator::PaginatedStream3;

mod pool_api;
pub use pool_api::PoolApi;
