mod api;
mod operation;
mod table_source;
#[cfg(feature = "vista")]
pub mod vista;

pub use api::{PaginationParams, ResponseShape, RestApi, RestApiBuilder};
pub(crate) use operation::condition_to_query_param;
pub use operation::eq_condition;
#[cfg(feature = "vista")]
pub use vista::{RestApiTableShell, RestApiVistaFactory};
