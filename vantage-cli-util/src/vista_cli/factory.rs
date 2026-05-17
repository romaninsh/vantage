//! `ModelFactory` and `Renderer` traits — the per-backend injection
//! points for the runner.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;
use vantage_vista::Vista;

use super::token::{AggregateOp, Mode};

/// Resolves model identifiers (singular/plural names, locators) to
/// `Vista`s. Implemented per-backend.
pub trait ModelFactory {
    /// Resolve a model name (e.g. `users` or `user`).
    /// Singular names should return [`Mode::Single`], plural
    /// [`Mode::List`]. Returns `None` for unknown names.
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)>;

    /// Resolve a universal resource locator — anything with a scheme,
    /// not just AWS ARNs. Examples:
    /// `arn:aws:iam::123:user/alice`, `user:abc123` (SurrealDB Thing),
    /// `urn:isbn:0451450523`. Backends without a locator vocabulary
    /// can leave the default `None`.
    ///
    /// Defaults to forwarding to [`Self::for_arn`] so the older one-method
    /// trait surface keeps working.
    fn for_locator(&self, locator: &str) -> Option<Vista> {
        self.for_arn(locator)
    }

    /// Resolve an ARN. Kept for backwards-compatibility with factories
    /// that only knew ARNs; new factories should override
    /// [`Self::for_locator`] instead.
    fn for_arn(&self, _arn: &str) -> Option<Vista> {
        None
    }
}

/// Backend hook for printing list, single-record, and scalar results.
///
/// New `render_scalar` carries aggregate output (`@sum:price`, `@count`,
/// …). Implementors that don't care about aggregates can rely on the
/// default impl, which prints a one-liner to stdout.
pub trait Renderer {
    fn render_list(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    );

    fn render_record(
        &self,
        vista: &Vista,
        id: &str,
        record: &Record<CborValue>,
        relations: &[String],
    );

    /// Render the result of an aggregate token (`@sum:field` etc.).
    fn render_scalar(
        &self,
        _vista: &Vista,
        op: AggregateOp,
        field: Option<&str>,
        value: &CborValue,
    ) {
        match field {
            Some(f) => println!("{}({}) = {value:?}", op.name(), f),
            None => println!("{}() = {value:?}", op.name()),
        }
    }

    /// Side-channel signal that the runner hit a feature that's parsed
    /// but not yet wired through Vista. Default impl is silent;
    /// integration tests override to record exactly which stubs were
    /// reached.
    fn note_stub(&self, _what: &str) {}
}
