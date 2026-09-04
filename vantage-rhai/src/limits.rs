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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_profile_bounds_operations() {
        let mut engine = Engine::new();
        Limits::Ui.apply(&mut engine);
        let err = engine.run("while true {}").unwrap_err();
        assert!(matches!(
            *err,
            rhai::EvalAltResult::ErrorTooManyOperations(_)
        ));
    }

    #[test]
    fn both_profiles_bound_string_size() {
        for limits in [Limits::Ui, Limits::background()] {
            let mut engine = Engine::new();
            limits.apply(&mut engine);
            let err = engine.run(r#"let s = "x"; loop { s += s; }"#).unwrap_err();
            assert!(
                matches!(*err, rhai::EvalAltResult::ErrorDataTooLarge(..)),
                "{limits:?}: {err}"
            );
        }
    }

    #[test]
    fn zero_operations_is_not_unlimited() {
        // rhai treats 0 as "no ceiling"; the profile must never pass it on.
        let limits = Limits::Background { max_operations: 0 };
        assert_eq!(limits.max_operations(), 1);
        let mut engine = Engine::new();
        limits.apply(&mut engine);
        let err = engine.run("while true {}").unwrap_err();
        assert!(matches!(
            *err,
            rhai::EvalAltResult::ErrorTooManyOperations(_)
        ));
    }

    #[test]
    fn background_honours_caller_number() {
        let mut engine = Engine::new();
        Limits::Background { max_operations: 10 }.apply(&mut engine);
        assert!(
            engine
                .run("let x = 0; for i in 0..100 { x += i; }")
                .is_err()
        );
        let mut engine = Engine::new();
        Limits::background().apply(&mut engine);
        assert!(engine.run("let x = 0; for i in 0..100 { x += i; }").is_ok());
    }
}
