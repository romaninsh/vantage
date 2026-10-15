//! Topology detection against a live MongoDB.
//!
//! Requires a running replica set (`MONGODB_URL`, e.g.
//! `mongodb://localhost:27017/?directConnection=true`).

use bson::doc;
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

#[tokio::test]
async fn a_handle_built_from_a_client_does_not_probe_and_stays_unknown() {
    let client = mongodb::Client::with_uri_str("mongodb://127.0.0.1:1")
        .await
        .unwrap();
    let db = MongoDB::new(client, "vantage_topology");

    assert_eq!(db.topology(), Topology::Unknown);
    assert!(!db.supports_change_streams());
}

#[tokio::test]
async fn connect_to_an_unreachable_server_leaves_the_topology_unknown() {
    let db = MongoDB::connect(
        "mongodb://127.0.0.1:1/?serverSelectionTimeoutMS=300",
        "vantage_topology",
    )
    .await
    .expect("connecting only parses the string; the probe failing is not an error");

    assert_eq!(db.topology(), Topology::Unknown);
    assert!(!db.supports_change_streams());
}

#[tokio::test]
async fn connect_refuses_a_string_that_is_no_connection_string() {
    let err = MongoDB::connect("not a connection string", "vantage_topology")
        .await
        .expect_err("an unparseable string must be refused");
    assert!(err.to_string().contains("connect"), "{err}");
}

#[test]
fn hello_with_a_set_name_is_a_replica_set() {
    let reply = doc! { "isWritablePrimary": true, "setName": "rs0" };
    assert_eq!(Topology::from_hello(&reply), Topology::ReplicaSet);
}

#[test]
fn hello_from_a_mongos_is_sharded() {
    let reply = doc! { "isWritablePrimary": true, "msg": "isdbgrid" };
    assert_eq!(Topology::from_hello(&reply), Topology::Sharded);
}

#[test]
fn a_hello_without_set_name_or_mongos_marker_is_standalone() {
    let reply = doc! { "isWritablePrimary": true, "maxBsonObjectSize": 16_777_216 };
    assert_eq!(Topology::from_hello(&reply), Topology::Standalone);
}

#[test]
fn only_replica_sets_and_sharded_clusters_support_change_streams() {
    assert!(Topology::ReplicaSet.supports_change_streams());
    assert!(Topology::Sharded.supports_change_streams());
    assert!(!Topology::Standalone.supports_change_streams());
    assert!(!Topology::Unknown.supports_change_streams());
}
