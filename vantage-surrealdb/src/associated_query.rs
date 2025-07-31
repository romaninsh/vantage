//! AssociatedQuery combines information about any the database
//!
//!
use std::{marker::PhantomData, sync::Arc};

use vantage_expressions::result::QueryResult;

use crate::{protocol::SurrealQueriable, surrealdb::SurrealDB};

// SurrealQuery contains Queryable and returns a specific result
struct AssociatedQuery<Q: SurrealQueriable, R: QueryResult> {
    db: SurrealDB,
    t: PhantomData<R>,
    q: Q,
}
