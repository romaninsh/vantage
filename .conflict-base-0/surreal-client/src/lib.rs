//! SurrealDB connectivity layer
//!
//! This module provides a comprehensive interface for connecting to and interacting
//! with SurrealDB instances via HTTP and WebSocket protocols.

pub mod client;
pub mod connection;
pub mod engine;
pub mod engines;
pub mod error;
pub mod mocks;
pub mod params;
#[cfg(feature = "pool")]
pub mod pool;
pub mod record;
pub mod rpc;
pub mod session;

// Re-export the main client from the parent module
pub use client::SurrealClient;
pub use connection::SurrealConnection;

pub use engine::Engine;
pub use engines::{DebugEngine, WsCborEngine, WsEngine};
pub use error::{Result, SurrealError};
pub use mocks::{MockSurrealEngine, SurrealMockBuilder};
pub use record::{RecordId, RecordIdValue, RecordRange, Table};
pub use rpc::{RpcMessage, RpcResponse};
pub use session::SessionState;
