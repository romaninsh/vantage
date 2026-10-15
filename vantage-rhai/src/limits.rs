//! Closed set of resource profiles. There is no unlimited engine.

use rhai::Engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limits {
    /// Anything the UI thread waits on.
    Ui,
    /// Anything under `spawn_blocking`: workers, cmd scripts, imports, faker ticks.
    Background { max_operations: u64 },
}

pub const UI_MAX_OPERATIONS: u64 = 500_000;
pub const BACKGROUND_MAX_OPERATIONS: u64 = 50_000_000;

impl Limits {
    pub fn background() -> Limits {
        Limits::Background {
            max_operations: BACKGROUND_MAX_OPERATIONS,
        }
    }

    /// The operation ceiling, never zero: rhai reads
    /// `set_max_operations(0)` as *unlimited*, which would quietly undo the
    /// one guarantee this type exists to make.
    pub fn max_operations(&self) -> u64 {
        match self {
            Limits::Ui => UI_MAX_OPERATIONS,
            Limits::Background { max_operations } => (*max_operations).max(1),
        }
    }

    pub fn apply(&self, engine: &mut Engine) {
        engine.set_max_operations(self.max_operations());
        engine.set_max_string_size(8 * 1024 * 1024);
        engine.set_max_array_size(1_000_000);
        engine.set_max_map_size(100_000);
        engine.set_max_call_levels(64);
        engine.set_max_expr_depths(256, 256);
    }
}
