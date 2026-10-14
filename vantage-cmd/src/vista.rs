//! Vista integration for `vantage-cmd`: the YAML-facing spec types, the
//! factory (typed-table *and* YAML construction), and the `TableShell`.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::CmdVistaFactory;
pub use spec::{CmdBlock, CmdColumnExtras, CmdTableExtras, CmdVistaSpec};
