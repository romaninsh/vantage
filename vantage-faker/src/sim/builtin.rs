//! Built-in sim scripts shipped with vantage-faker, embedded at compile time
//! from `vantage-faker/sims/*.rhai`. A `sims:` entry's `script: "builtin:<name>"`
//! resolves through [`builtin`] instead of naming an `!include`d file.

/// Names [`builtin`] resolves, in the order a "known:" error list shows them.
pub const BUILTINS: &[&str] = &["fifo", "pulse", "flight", "folder_tree"];

/// The Rhai source of the built-in script named `name`, or `None` if `name`
/// is not one of [`BUILTINS`].
pub fn builtin(name: &str) -> Option<&'static str> {
    match name {
        "fifo" => Some(include_str!("../../sims/fifo.rhai")),
        "pulse" => Some(include_str!("../../sims/pulse.rhai")),
        "flight" => Some(include_str!("../../sims/flight.rhai")),
        "folder_tree" => Some(include_str!("../../sims/folder_tree.rhai")),
        _ => None,
    }
}
