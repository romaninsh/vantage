//! Contract checks for `Vista` drivers (feature `contract`).
//!
//! Every driver runs these against its own fixture, so the promises a
//! [`Vista`](crate::Vista) makes are stated once and held everywhere. A check
//! runs every probe, then panics once, listing every broken rule.
//!
//! - [`check_advertised`]: every advertised read, narrowing and traversal
//!   [`Capability`] is backed by a working method. Reads
//!   and narrowing must succeed on a valid fixture; traversal and native
//!   conditions only must not be refused (a missing join value or a foreign
//!   condition type is not a contract break). On a fixture with rows, search,
//!   order and operator filters must also change what is read. A condition
//!   on a dotted path into a declared column narrows or is refused as
//!   `Unimplemented`, never failing with a plain error; whether an
//!   accepted one narrows is checked only on a fixture with a row holding a
//!   map in a declared column. Every shell that lists must answer the
//!   leading window, advertised `Window` or not: when `list` works,
//!   `fetch_window(0, n)` returns `min(n, listed)` rows of the set, the first
//!   in the set's order when an order is applied. Narrowing probes run on
//!   clones, so the vista passed in is left as it was.
//! - [`check_narrowing`]: the narrowing half of `check_advertised` alone.
//! - [`check_advertised_writes`]: the write capabilities, held to the lenient
//!   rule. Run it only on a disposable fixture, since a successful probe may
//!   store a row.
//! - [`check_clone`]: a clone carries its narrowing, and narrowing a clone
//!   leaves the original alone.
//!
//! - [`check_transactions`]: writes through a [`Transaction`](crate::Transaction)
//!   land together or not at all, on tables a [`TxFixture`] hands out fresh.
//!   Each probe has its own deadline, so a hang names the probe that hung.
//!
//! Each driver's contract test file runs `check_clone` and
//! `check_advertised`, plus `check_advertised_writes` where its fixture is
//! disposable, and `check_transactions` where its backend has transactions.

mod advertised;
mod clone;
mod narrowing;
mod transactions;
mod window;
mod writes;

pub use advertised::check_advertised;
pub use clone::check_clone;
pub use narrowing::check_narrowing;
pub use transactions::{TxFixture, TxTables, check_transactions};
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
