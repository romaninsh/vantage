//! The refusal of a dotted path a shell can't evaluate.

use vantage_core::{VantageError, error};

use crate::metadata::VistaMetadata;

/// The refusal of a condition on a dotted column (`root.leaf`, or an
/// imported `author.name`) a shell can't evaluate: an `Unimplemented`
/// error naming the column. It is not an [`unsupported`](crate::unsupported)
/// refusal, since no capability describes paths.
pub(crate) fn refuse_path(column: &str) -> VantageError {
    error!("this shell doesn't evaluate a dotted path", column = column).mark_unimplemented()
}

/// The error for a condition on `column`, which the shell doesn't take as
/// one of its own columns.
///
/// - A dotted column the shell can't evaluate is refused as
///   `Unimplemented`, for the caller to evaluate: a path whose root
///   `metadata` declares, a dotted column `metadata` declares whole (an
///   imported `author.name`), or any dotted column when `metadata` declares
///   no columns. Being a refusal,
///   [`Vista::with_query`](crate::Vista::with_query) hands the clause back
///   as residual, for the caller to evaluate over the rows it reads with
///   [`Condition::matches`](crate::Condition::matches);
///   [`Vista::with_condition`](crate::Vista::with_condition) returns it as
///   an error.
/// - Anything else: a plain unknown-column error, so an unknown column is
///   never evaluated locally.
pub fn condition_column_error(metadata: &VistaMetadata, column: &str) -> VantageError {
    let columns = &metadata.columns;
    let nested = column.split_once('.').is_some_and(|(root, _)| {
        columns.is_empty() || columns.contains_key(column) || columns.contains_key(root)
    });
    if nested {
        refuse_path(column)
    } else {
        error!("Unknown column for condition", field = column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Column;

    #[test]
    fn a_path_into_a_declared_root_is_refused() {
        let metadata = VistaMetadata::new()
            .with_column(Column::new("dims", "map"))
            .with_column(Column::new("author.name", "String"));
        let refused = condition_column_error(&metadata, "dims.w");
        assert!(refused.is_unimplemented(), "{refused:?}");
        assert!(refused.to_string().contains("dotted path"), "{refused}");
        for unknown in ["colour", "colour.hue", "author"] {
            let err = condition_column_error(&metadata, unknown);
            assert!(
                !err.is_unimplemented() && !err.is_unsupported(),
                "{unknown}"
            );
            assert!(err.to_string().contains("Unknown column"), "{err}");
        }
    }

    #[test]
    fn a_declared_dotted_column_is_refused_as_a_path() {
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String"))
            .with_column(Column::new("author.name", "String"));
        let refused = condition_column_error(&metadata, "author.name");
        assert!(refused.is_unimplemented(), "{refused:?}");
        assert!(refused.to_string().contains("dotted path"), "{refused}");
    }

    #[test]
    fn a_schemaless_shell_refuses_every_path() {
        let err = condition_column_error(&VistaMetadata::new(), "colour.hue");
        assert!(err.is_unimplemented(), "{err:?}");
        let flat = condition_column_error(&VistaMetadata::new(), "colour");
        assert!(!flat.is_unimplemented(), "{flat:?}");
    }
}
