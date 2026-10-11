//! A resilient async HTTP client for the Vantage API backends: one
//! [`ResilientClient`] per remote API, with bounded parallelism, retry, a
//! circuit breaker, a rate limit and auth refresh. See [`resilient`].
#![warn(missing_docs)]

pub mod resilient;
pub use resilient::{
    AuthFuture, AuthRefresher, BreakerMode, BreakerState, CallPolicy, ClientError, ErrorKind,
    ResilientClient, ResilientClientBuilder, RetryMode, RetryPolicy, TransportEvent,
    TransportObserver,
};
