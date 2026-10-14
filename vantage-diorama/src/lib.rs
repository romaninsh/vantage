#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), deny(clippy::unimplemented, clippy::todo))]

pub mod aggregate;
mod augment;
// The debug tap and the live-instance tallies run in every build, because the
// crate calls them throughout; the `diagnostics` feature publishes them.
#[cfg(feature = "diagnostics")]
pub mod debug;
#[cfg(not(feature = "diagnostics"))]
mod debug;
mod dio;
mod lens;
mod ops;
#[cfg(feature = "rhai")]
pub mod rhai;
mod scenery;
mod servo;
#[cfg(feature = "diagnostics")]
pub mod stats;
#[cfg(not(feature = "diagnostics"))]
mod stats;

pub use augment::{Augmentation, BuildFn, Detail, Fetch, FetchFn, MergeRule, Source};
pub use dio::{
    DeriveBuilder, Dio, DioBuilder, DioEvent, Fetched, Generation, ImportOutcome, WeakDio,
    WriteCapabilities,
};
#[cfg(feature = "diagnostics")]
pub use dio::{DioDiagnostics, SceneryDiagnostic};
pub use lens::{
    Cache, CacheBackend, CacheStatus, CacheTable, ChunkSink, Lens, LensBuilder, Loader, LoaderCx,
    MemoryCache, MemoryCacheTable, RedbCache, RedbCacheTable, Refresher, Tuning,
};
pub use ops::{ChangeEvent, ChangeFlash, FlashKind, FlashOutcome, FlashRejection};
pub use scenery::{
    Clause, EnrichedRecord, FilterOrigin, LoadFailure, LoadState, RecordScenery,
    RecordSceneryBuilder, RecordStatus, RowStatus, RowStatusSummary, Sugar, SugarFn, SugarFuture,
    TableScenery, TableSceneryBuilder, ValueScenery, ValueSceneryBuilder, ValueStatus, ViewCount,
    ViewFilter, ViewState, ViewStats,
};
pub use servo::{IdStrategy, Servo, ServoStatus};

/// Common imports for working with vantage-diorama.
///
/// ```
/// use vantage_diorama::prelude::*;
/// ```
pub mod prelude {
    pub use crate::aggregate::Aggregation;
    pub use crate::{
        ChangeEvent, ChangeFlash, Dio, Lens, RecordScenery, Servo, TableScenery, ValueScenery,
    };
}
