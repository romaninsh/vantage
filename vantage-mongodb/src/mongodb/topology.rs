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

#[cfg(test)]
mod tests {
    use super::*;
    use bson::doc;

    #[test]
    fn hello_with_set_name_is_replica_set() {
        let reply = doc! { "isWritablePrimary": true, "setName": "rs0" };
        assert_eq!(Topology::from_hello(&reply), Topology::ReplicaSet);
    }

    #[test]
    fn hello_from_mongos_is_sharded() {
        let reply = doc! { "isWritablePrimary": true, "msg": "isdbgrid" };
        assert_eq!(Topology::from_hello(&reply), Topology::Sharded);
    }

    #[test]
    fn plain_hello_is_standalone() {
        let reply = doc! { "isWritablePrimary": true, "maxBsonObjectSize": 16777216 };
        assert_eq!(Topology::from_hello(&reply), Topology::Standalone);
    }

    #[test]
    fn only_replica_set_and_sharded_support_change_streams() {
        assert!(Topology::ReplicaSet.supports_change_streams());
        assert!(Topology::Sharded.supports_change_streams());
        assert!(!Topology::Standalone.supports_change_streams());
        assert!(!Topology::Unknown.supports_change_streams());
    }
}
