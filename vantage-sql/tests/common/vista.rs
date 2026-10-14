//! The vista suite each SQL backend runs: typed and YAML-built vistas over a
//! seeded product table (`vista/product.rs`), and YAML references and `base:`
//! derivations over a client/orders pair (`vista/client_orders.rs`).
//!
//! A backend's `vista` module imports `vantage_dataset::prelude::*` and
//! `vantage_vista::VistaFactory`, and defines `TestResult` and two fixtures, each
//! taking a `suffix` that keeps tests apart where the database is shared:
//!
//! - `async fn product(suffix: &str) -> (DB, Table<DB, EmptyEntity>)`: a
//!   product table with `id`, `name`, `price` and `is_deleted`, holding
//!   `a`/Alpha/10, `b`/Beta/20 (deleted) and `c`/Gamma/30;
//! - `async fn clients_orders(suffix: &str) -> (DB, String, String)`:
//!   clients `alice` and `bob`, orders `o1`, `o2` (alice, 10 and 20) and
//!   `o3` (bob, 30), returning the client and orders table names.
//!
//! It then invokes `vista_tests!(SpecType, "block")`, `block` being the
//! dialect's YAML driver key.

#[macro_use]
#[path = "vista/client_orders.rs"]
mod client_orders;
#[macro_use]
#[path = "vista/product.rs"]
mod product;

macro_rules! vista_tests {
    ($spec:ty, $block:literal) => {
        vista_product_tests!($block);
        vista_client_orders_tests!($spec, $block);
    };
}
