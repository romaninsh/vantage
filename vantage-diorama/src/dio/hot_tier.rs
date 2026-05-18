/// In-memory hot cache of recently-touched `Arc<EnrichedRecord>` values.
///
/// Stage 5 wires this to a `moka::future::Cache`. Stage 1 holds the
/// placeholder so `DioInner` has a stable field name.
pub struct HotTier {
    _private: (),
}

impl Default for HotTier {
    fn default() -> Self {
        Self::new()
    }
}

impl HotTier {
    pub fn new() -> Self {
        Self { _private: () }
    }
}
