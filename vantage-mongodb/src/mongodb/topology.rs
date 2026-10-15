//! Deployment topology, detected once from the server's `hello` reply.

use bson::Document;

/// What kind of MongoDB deployment a connection talks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Topology {
    /// A single `mongod` outside any replica set; it has no oplog.
    Standalone,
    /// A replica-set member (a single-node set included).
    ReplicaSet,
    /// A `mongos` router in front of a sharded cluster.
    Sharded,
    /// The `hello` probe failed, so the deployment kind is not known.
    Unknown,
}

impl Topology {
    /// Classify a `hello` reply: a `mongos` answers `msg: "isdbgrid"`, a
    /// replica-set member reports its `setName`, anything else is standalone.
    pub fn from_hello(reply: &Document) -> Self {
        if reply.get_str("msg") == Ok("isdbgrid") {
            Self::Sharded
        } else if reply.contains_key("setName") {
            Self::ReplicaSet
        } else {
            Self::Standalone
        }
    }

    /// Change streams need an oplog, which replica sets and sharded
    /// clusters have.
    pub fn supports_change_streams(self) -> bool {
        matches!(self, Self::ReplicaSet | Self::Sharded)
    }
}
