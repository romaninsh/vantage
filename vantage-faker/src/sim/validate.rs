//! Load-time checks on sim definitions.

use std::collections::HashSet;
use std::sync::LazyLock;

use vantage_rhai::{Host, Limits, Mode};

use super::{MAX_LIVE, SIM_EXPR_DEPTHS, SimDef};

impl SimDef {
    /// Report a def the engine cannot run: an empty name, table or script, a
    /// script that does not compile, `max: 0`, a `burst` above `max`, a
    /// negative or non-finite `rate_per_min`, a non-positive or non-finite
    /// `clock`, or a `max` above [`MAX_LIVE`].
    pub fn validate(&self) -> Result<(), String> {
        let name = &self.name;
        if name.trim().is_empty() {
            return Err("sim: name is empty".into());
        }
        if self.table.trim().is_empty() {
            return Err(format!("sim {name}: table is empty"));
        }
        if self.script.trim().is_empty() {
            return Err(format!("sim {name}: script is empty"));
        }
        let spawn = &self.spawn;
        if spawn.max == 0 {
            return Err(format!("sim {name}: max must be at least 1"));
        }
        if spawn.max > MAX_LIVE {
            return Err(format!(
                "sim {name}: max {} is above the limit of {MAX_LIVE} live sims",
                spawn.max
            ));
        }
        if spawn.burst > spawn.max {
            return Err(format!(
                "sim {name}: burst {} is above max {}",
                spawn.burst, spawn.max
            ));
        }
        if !spawn.rate_per_min.is_finite() || spawn.rate_per_min < 0.0 {
            return Err(format!(
                "sim {name}: rate {} must be finite and not negative",
                spawn.rate_per_min
            ));
        }
        if !self.clock.is_finite() || self.clock <= 0.0 {
            return Err(format!(
                "sim {name}: clock {} must be finite and positive",
                self.clock
            ));
        }
        compile_host()
            .ast_uncached(Mode::Script, &self.script)
            .map_err(|e| format!("sim {name}: script does not compile: {e}"))?;
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
pub(super) fn validate_all(defs: &[SimDef], tables: &HashSet<&str>) -> Result<(), String> {
    let mut names = HashSet::new();
    for def in defs {
        def.validate()?;
        if !names.insert(def.name.as_str()) {
            return Err(format!("sim {}: defined twice", def.name));
        }
        if !tables.contains(def.table.as_str()) {
            return Err(format!(
                "sim {}: table {} is not a table of this datasource",
                def.name, def.table
            ));
        }
    }
    let total: usize = defs.iter().map(|d| d.spawn.max).sum();
    if total > MAX_LIVE {
        return Err(format!(
            "sims: max adds up to {total}, above the limit of {MAX_LIVE} live sims"
        ));
    }
    Ok(())
}
