mod api;
mod operation;
mod table_source;
pub mod vista;

pub use api::{PaginationParams, ResponseShape, RestApi, RestApiBuilder};
pub(crate) use operation::condition_to_query_param;
pub use operation::eq_condition;
pub use vista::{
    AnyTableShell, NoApiExtras, RestApiTableShell, RestApiVistaFactory, RestApiVistaSpec,
};
