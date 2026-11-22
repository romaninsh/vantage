use mongodb::error::ErrorKind;
use vantage_core::{VantageError, error};

/// `message` with the driver's details, marked unavailable when the server
/// could not be reached: a network or IO failure, no server selected in time,
/// or a cleared connection pool. A refusal by the server (a write error, a
/// validator, a duplicate key) is left unmarked.
pub(crate) fn driver_error(message: &'static str, e: &mongodb::error::Error) -> VantageError {
    let err = error!(message, details = e.to_string());
    if is_unavailable(e) {
        err.mark_unavailable()
    } else {
        err
    }
}

fn is_unavailable(e: &mongodb::error::Error) -> bool {
    matches!(
        &*e.kind,
        ErrorKind::Io(_)
            | ErrorKind::ServerSelection { .. }
            | ErrorKind::ConnectionPoolCleared { .. }
    )
}
