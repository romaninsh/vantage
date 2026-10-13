#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), deny(clippy::unimplemented, clippy::todo))]

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
pub mod scenery;
mod servo;
#[cfg(feature = "diagnostics")]
pub mod stats;
#[cfg(not(feature = "diagnostics"))]
mod stats;

pub use augment::{Augmentation, BuildFn, Detail, Fetch, FetchFn, MergeRule, Source};
pub use dio::{
    Dio, DioBuilder, DioEvent, Fetched, Generation, ImportOutcome, WeakDio, WriteCapabilities,
};
#[cfg(feature = "diagnostics")]
pub use dio::{DioDiagnostics, SceneryDiagnostic};
pub use lens::{
    Cache, CacheBackend, CacheStatus, CacheTable, ChunkSink, Lens, LensBuilder, Loader, LoaderCx,
    MemoryCache, MemoryCacheTable, RedbCache, RedbCacheTable, Refresher, Tuning,
};
pub use ops::{ChangeEvent, ChangeFlash, FlashKind, FlashOutcome, FlashRejection};
pub use scenery::sugar::{Sugar, SugarFn, SugarFuture};
pub use scenery::{
    Aggregate, CappedScenery, Clause, CustomAggregate, EnrichedRecord, FilterOrigin, LoadFailure,
    LoadState, OpCondition, RecordScenery, RecordStatus, RowStatus, RowStatusSummary, SortDir,
    TableScenery, TableSceneryBuilder, ValueScenery, ValueSceneryBuilder, ValueStatus, ViewCount,
    ViewFilter, ViewState, ViewStats, boxed_custom_aggregate,
};
pub use servo::{IdStrategy, Servo, ServoStatus};

/// Common imports for working with vantage-diorama.
///
/// ```
/// use vantage_diorama::prelude::*;
/// ```
pub mod prelude {
    pub use crate::augment::{Augmentation, Detail, Fetch, MergeRule, Source};
    pub use crate::dio::{Dio, DioEvent, Generation, WriteCapabilities};
    pub use crate::lens::{
        Cache, CacheBackend, CacheStatus, CacheTable, Lens, Loader, LoaderCx, RedbCache, Refresher,
        Tuning,
    };
    pub use crate::ops::{ChangeEvent, ChangeFlash, FlashKind, FlashOutcome};
    pub use crate::scenery::{
        EnrichedRecord, RecordScenery, RecordStatus, RowStatus, SortDir, TableScenery, ValueScenery,
    };
    pub use crate::servo::{IdStrategy, Servo, ServoStatus};
}
