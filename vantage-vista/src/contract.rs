//! Contract checks for `Vista` drivers (feature `contract`).
//!
//! Every driver runs these against its own fixture, so the promises a
//! [`Vista`](crate::Vista) makes are stated once and held everywhere. A check
//! runs every probe, then panics once, listing every broken rule.
//!
//! - [`check_advertised`]: every advertised read, narrowing and traversal
//!   [`Capability`](crate::Capability) is backed by a working method. Reads
//!   and narrowing must succeed on a valid fixture; traversal and native
//!   conditions only must not be refused (a missing join value or a foreign
//!   condition type is not a contract break). On a fixture with rows, search,
//!   order and operator filters must also change what is read. Narrowing
//!   probes run on clones, so the vista passed in is left as it was.
//! - [`check_narrowing`]: the narrowing half of `check_advertised` alone.
//! - [`check_advertised_writes`]: the write capabilities, held to the lenient
//!   rule. Run it only on a disposable fixture, since a successful probe may
//!   store a row.
//! - [`check_clone`]: a clone carries its narrowing, and narrowing a clone
//!   leaves the original alone.
//!
//! Each driver's contract test file runs `check_clone` and
//! `check_advertised`, plus `check_advertised_writes` where its fixture is
//! disposable.

mod advertised;
mod clone;
mod narrowing;
mod writes;

pub use advertised::check_advertised;
pub use clone::check_clone;
pub use narrowing::check_narrowing;
pub use writes::check_advertised_writes;

use vantage_core::Result;

use crate::Capability;

/// Collects the broken rules of one check, so a driver sees all of them in
/// one run.
#[derive(Default)]
struct Probe {
    failures: Vec<String>,
}

impl Probe {
    /// Lenient rule: `result` may be any error except a refusal
    /// (`Unsupported`/`Unimplemented`).
    fn check<T>(&mut self, capability: Capability, method: &str, result: &Result<T>) {
        if let Err(e) = result
            && (e.is_unsupported() || e.is_unimplemented())
        {
            self.fail(format!(
                "`{capability}` is advertised but `{method}` refuses it: {e}"
            ));
        }
    }

    /// Strict rule: on a valid fixture, `result` must be `Ok`.
    fn check_ok<T>(&mut self, capability: Capability, method: &str, result: &Result<T>) {
        match result {
            Err(e) if e.is_unsupported() || e.is_unimplemented() => self.fail(format!(
                "`{capability}` is advertised but `{method}` refuses it: {e}"
            )),
            Err(e) => self.fail(format!(
                "`{capability}` is advertised but `{method}` fails on a valid fixture: {e}"
            )),
            Ok(_) => {}
        }
    }

    fn fail(&mut self, failure: String) {
        self.failures.push(failure);
    }

    /// Panics once, one failure per line, when any probe failed.
    #[track_caller]
    fn finish(self) {
        if !self.failures.is_empty() {
            panic!(
                "{} contract failure(s):\n{}",
                self.failures.len(),
                self.failures.join("\n")
            );
        }
    }
}
