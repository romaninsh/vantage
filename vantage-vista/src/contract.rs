//! Contract checks for `Vista` drivers (feature `contract`).
//!
//! Every driver runs these against its own fixture, so the promises a
//! [`Vista`](crate::Vista) makes are stated once and held everywhere. A check
//! runs every probe, then panics once, listing every broken rule.
//!
//! - [`check_advertised`]: every capability flag the vista advertises is
//!   backed by a working method. Reads must succeed on a valid fixture;
//!   reference traversal must not answer `Unsupported` or `Unimplemented`
//!   (other errors, such as a missing join value, are not contract breaks).
//! - [`check_narrowing`]: the page-size and narrowing half of
//!   `check_advertised`, for a vista the caller can throw away.
//!   `check_advertised` runs it on a clone, and skips it when the shell can't
//!   be cloned.
//! - [`check_advertised_writes`]: the write flags, held to the lenient rule.
//!   Run it only on a disposable fixture, since a successful probe may store
//!   a row.
//! - [`check_clone`]: `clone_shell` exists, a clone carries its narrowing,
//!   and narrowing a clone leaves the original alone.
//!
//! Each driver's contract test file runs `check_clone` and
//! `check_advertised`, plus `check_narrowing` while its shell can't be
//! cloned, and `check_advertised_writes` where its fixture is disposable. A
//! shell that can't be cloned fails `check_clone`, not `check_advertised`.

mod advertised;
mod clone;
mod narrowing;
mod writes;

pub use advertised::check_advertised;
pub use clone::check_clone;
pub use narrowing::check_narrowing;
pub use writes::check_advertised_writes;

use vantage_core::Result;

/// Collects the broken rules of one check, so a driver sees all of them in
/// one run.
#[derive(Default)]
struct Probe {
    failures: Vec<String>,
}

impl Probe {
    /// Lenient rule: `result` may be any error except a refusal
    /// (`Unsupported`/`Unimplemented`).
    fn check<T>(&mut self, capability: &str, method: &str, result: &Result<T>) {
        if let Err(e) = result
            && (e.is_unsupported() || e.is_unimplemented())
        {
            self.fail(format!(
                "`{capability}` is advertised but `{method}` refuses it: {e}"
            ));
        }
    }

    /// Strict rule: on a valid fixture, `result` must be `Ok`.
    fn check_ok<T>(&mut self, capability: &str, method: &str, result: &Result<T>) {
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
