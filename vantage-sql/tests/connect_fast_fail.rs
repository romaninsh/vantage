//! A refused server must fail `connect` at once, not after the pool's
//! 30-second acquire deadline. No database needed: the port comes
//! from a listener that is closed before the connect runs.
#![cfg(any(feature = "postgres", feature = "mysql"))]

use std::time::{Duration, Instant};

/// An ephemeral port nothing listens on.
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_connect_fails_fast_when_refused() {
    let url = format!("postgres://u:p@127.0.0.1:{}/db", closed_port());
    let started = Instant::now();
    let result = vantage_sql::postgres::PostgresDB::connect(&url).await;
    assert!(result.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}

#[cfg(feature = "mysql")]
#[tokio::test]
async fn mysql_connect_fails_fast_when_refused() {
    let url = format!("mysql://u:p@127.0.0.1:{}/db", closed_port());
    let started = Instant::now();
    let result = vantage_sql::mysql::MysqlDB::connect(&url).await;
    assert!(result.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}
