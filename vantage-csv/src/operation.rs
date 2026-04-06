//! CSV condition operation constants.
//!
//! Template markers for condition operations. CSV's in-memory evaluator
//! matches on these to know which operation to apply. These match the
//! default templates in `Operation<T>`.

pub const OP_EQ: &str = "{} = {}";
pub const OP_IN: &str = "{} IN ({})";
