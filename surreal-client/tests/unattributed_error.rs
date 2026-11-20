//! An error the server can't tie to a request (it couldn't decode the request, so it couldn't
//! read its id) fails the waiting request at once instead of leaving it to its deadline.
//!
//! Requires SurrealDB at localhost:8000 (root/root), like the other live tests.

use std::time::Duration;

use ciborium::Value as CborValue;
use surreal_client::SurrealConnection;

#[tokio::test]
async fn an_undecodable_request_fails_at_once() {
    let db = SurrealConnection::dsn("cbor://root:root@localhost:8000/test/unattributed_error")
        .expect("dsn")
        .connect()
        .await
        .expect("connect");
    // SurrealDB decodes no tag 101, so its reply carries no request id.
    let vars = CborValue::Map(vec![(
        CborValue::Text("v".into()),
        CborValue::Tag(101, Box::new(CborValue::Text("16:01:00".into()))),
    )]);
    let reply = tokio::time::timeout(Duration::from_secs(2), db.query_cbor("RETURN $v", vars))
        .await
        .expect("the request fails at once, not at its deadline");
    assert!(reply.is_err(), "{reply:?}");
}
