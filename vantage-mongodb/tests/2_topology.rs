//! Topology detection against a live MongoDB.
//!
//! Requires a running replica set (`MONGODB_URL`, e.g.
//! `mongodb://localhost:27017/?directConnection=true`).

use vantage_mongodb::{MongoDB, Topology};

fn mongo_url() -> String {
    std::env::var("MONGODB_URL")
        .unwrap_or_else(|_| "mongodb://localhost:27017/?directConnection=true".into())
}

#[tokio::test]
async fn connect_detects_replica_set() {
    let db = MongoDB::connect(&mongo_url(), "vantage_topology")
        .await
        .expect("Failed to connect to MongoDB");

    assert_eq!(db.topology(), Topology::ReplicaSet);
    assert!(db.supports_change_streams());
}
