use std::sync::Arc;

use vantage_vista::VistaCapabilities;

use super::DioInner;

/// `TableShell` impl that backs the Vista returned by `Dio::vista()`.
///
/// Stage 1 ships the struct only — the impl lives in
/// [`super::impls::table_shell`] and returns `Unsupported` for every
/// method. Stage 2 wires reads through the cache and writes onto the
/// write queue.
pub struct DioShell {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) capabilities: VistaCapabilities,
}

impl DioShell {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        let capabilities = VistaCapabilities::default();
        Self { dio, capabilities }
    }
}
