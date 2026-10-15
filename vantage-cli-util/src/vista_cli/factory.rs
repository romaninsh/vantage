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

    /// Resolve a locator: anything with a scheme, such as
    /// `arn:aws:iam::123:user/alice`, `user:abc123` or `urn:isbn:0451450523`.
    /// Forwards to [`Self::for_arn`] by default, so factories that only
    /// know ARNs keep working.
    fn for_locator(&self, locator: &str) -> Option<Vista> {
        self.for_arn(locator)
    }

    /// Resolve an ARN. Returns `None` by default; new factories override
    /// [`Self::for_locator`] instead.
    fn for_arn(&self, _arn: &str) -> Option<Vista> {
        None
    }
}

/// Backend hook for printing list, single-record, and scalar results.
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

    /// Render the result of an aggregate token (`@sum:field` etc.). The
    /// default prints a one-liner to stdout.
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

    /// Called when the runner hits a feature that parses but isn't wired
    /// through Vista yet. Silent by default.
    fn note_stub(&self, _what: &str) {}
}
