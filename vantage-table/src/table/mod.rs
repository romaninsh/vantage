//! Table is our generic implementation of remote record collection, that may have columns and supports
//! column operations (like sorting or filtering).
//!
//! Table<E, S> is defined over E=Entity and S=TableSource
//!
//! In practice, it's benificial to use a single struct that implements:
//!  - TableSource (defined in this crate)
//!  - QuerySource (defined in vantage-expressions)
//!  - SelectSource (defined in vantage-expessions)
//!
//! Table<_, S: QuerySource> and Table<_, S: SelectSource> will define
//! additional properties.
//!
//!
//! Additionally this crate defines TableLike trait and AnyTable, that
//! proides type-erased version of TableLike.
//! A table abstraction defined over a datasource and entity

pub mod base;
pub use base::*;

pub mod impls;
pub use impls::*;

pub mod sets;
pub use sets::*;
