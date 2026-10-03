//! Load-time checks on sim definitions.

use std::collections::HashSet;
use std::sync::LazyLock;

use vantage_core::{Context, Result, error};
use vantage_rhai::{Host, Limits, Mode};

use super::{MAX_LIVE, SIM_EXPR_DEPTHS, SimDef};

impl SimDef {
    /// Report a def the engine cannot run: an empty name, table or script, a
    /// script that does not compile, `max: 0`, a `burst` above `max`, a
    /// negative or non-finite `rate_per_min`, a non-positive or non-finite
    /// `clock`, `ops: 0`, or a `max` above [`MAX_LIVE`]. Every error but the
    /// empty name carries the sim's name as `sim`.
    pub fn validate(&self) -> Result<()> {
        let name = &self.name;
        if name.trim().is_empty() {
            return Err(error!("Sim name is empty"));
        }
        if self.table.trim().is_empty() {
            return Err(error!("Sim table is empty", sim = name));
        }
        if self.script.trim().is_empty() {
            return Err(error!("Sim script is empty", sim = name));
        }
        let spawn = &self.spawn;
        if spawn.max == 0 {
            return Err(error!("Sim max must be at least 1", sim = name));
        }
        if spawn.max > MAX_LIVE {
            return Err(error!(
                "Sim max is above the live-sim limit",
                sim = name,
                max = spawn.max,
                limit = MAX_LIVE
            ));
        }
        if spawn.burst > spawn.max {
            return Err(error!(
                "Sim burst is above its max",
                sim = name,
                burst = spawn.burst,
                max = spawn.max
            ));
        }
        if !spawn.rate_per_min.is_finite() || spawn.rate_per_min < 0.0 {
            return Err(error!(
                "Sim rate must be finite and not negative",
                sim = name,
                rate = spawn.rate_per_min
            ));
        }
        if !self.clock.is_finite() || self.clock <= 0.0 {
            return Err(error!(
                "Sim clock must be finite and positive",
                sim = name,
                clock = self.clock
            ));
        }
        if self.ops == Some(0) {
            return Err(error!("Sim ops must be greater than 0", sim = name));
        }
        compile_host()
            .ast_uncached(Mode::Script, &self.script)
            .with_context(|| error!("Sim script does not compile", sim = name))?;
        Ok(())
    }
}

/// A verb-free host with the sims' parse limits, for syntax checks.
fn compile_host() -> &'static Host {
    static HOST: LazyLock<Host> = LazyLock::new(|| {
        Host::builder(Limits::background())
            .vocab_fn(|engine| {
                engine.set_max_expr_depths(SIM_EXPR_DEPTHS.0, SIM_EXPR_DEPTHS.1);
            })
            .build()
    });
    &HOST
}

/// Check a whole engine config: every def valid, names unique, every default
/// table known, and the sum of `max` within [`MAX_LIVE`].
pub(super) fn validate_all(defs: &[SimDef], tables: &HashSet<String>) -> Result<()> {
    let mut names = HashSet::new();
    for def in defs {
        def.validate()?;
        if !names.insert(def.name.as_str()) {
            return Err(error!("Sim is defined twice", sim = def.name));
        }
        if !tables.contains(def.table.as_str()) {
            return Err(error!(
                "Sim table does not exist",
                sim = def.name,
                table = def.table
            )
            .mark_not_found());
        }
    }
    let total: usize = defs.iter().map(|d| d.spawn.max).sum();
    if total > MAX_LIVE {
        return Err(error!(
            "Sims' max adds up to more than the live-sim limit",
            total = total,
            limit = MAX_LIVE
        ));
    }
    Ok(())
}
