/// Trait for associated queries that can be executed
///
/// This trait provides execution methods for queries that have been
/// associated with their datasource, allowing direct execution without
/// passing the datasource separately.
use vantage_core::Result;

#[async_trait::async_trait]
pub trait AssociatedQueryable<R> {
    /// Execute the query and return the result in the expected type
    async fn get(&self) -> Result<R>;
}
