/// Opaque description of a query a Scenery (or the Dio's vista facade) is
/// about to run against the cache. Used by `on_query` callbacks to opt
/// into eager fetches before the cache read happens.
///
/// Stage 1 is a placeholder newtype. Stage 5 fills the concrete fields
/// (conditions, sort, search, pagination) once Sceneries actually issue
/// queries.
#[derive(Debug, Clone, Default)]
pub struct QueryDescriptor {
    _private: (),
}

impl QueryDescriptor {
    pub fn new() -> Self {
        Self { _private: () }
    }
}
