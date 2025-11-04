/// Configuration for connection pooling
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    pub max_connections: u64,
    /// Connection timeout in seconds
    pub timeout: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            timeout: 30,
        }
    }
}
