//! SurrealDB connectivity layer
//!
//! This module provides a comprehensive interface for connecting to and interacting
//! with SurrealDB instances via HTTP and WebSocket protocols.

pub mod engine;
pub mod engines;
pub mod error;
pub mod params;
pub mod record;
pub mod rpc;
pub mod session;

// Re-export the main client from the parent module
pub use crate::client::SurrealClient;

pub use engine::Engine;
pub use engines::{HttpEngine, WsEngine};
pub use error::{Result, SurrealError};
pub use params::{ConnectParams, SigninParams, SignupParams};
pub use record::{RecordId, RecordIdValue, RecordRange, Table};
pub use rpc::{RpcMessage, RpcResponse};
pub use session::SessionState;
