#![doc = include_str!("../README.md")]
// Stage 1 is shape-only — fields/methods land their first callers in stages 2+.
#![allow(dead_code)]

pub mod augment;
pub mod composition;
pub mod dio;
pub mod error;
pub mod lens;
pub mod ops;
pub mod scenery;
pub mod servo;

pub use augment::{
    AugmentSpec, Augmentation, BuildFn, Detail, Fetch, FetchFn, FetchSpec, MergeRule, SetOp,
    Source, SourceSpec, lower_augment,
};
pub use composition::Diorama;
pub use dio::diagnostics::{DioDiagnostics, SceneryDiagnostic};
pub use dio::{Dio, DioEvent, DioShell, Generation, WriteCapabilities};
pub use error::{DioError, LensBuildError};
pub use lens::{
    Activity, ActivitySignal, CacheBackend, CacheStatus, CacheTable, ChunkRow, ChunkSink,
    DioCallback, DioEventCallback, DioFlashCallback, DioLoadChunkCallback,
    DioTotalProviderCallback, Lens, LensBuilder, LensCallbacks, LensDefaults, MemoryCache,
    MemoryCacheTable, RedbCache, RedbCacheTable,
};
pub use ops::{ChangeEvent, ChangeFlash, FlashKind, QueryDescriptor};
pub use scenery::{
    Aggregate, CappedScenery, CustomAggregate, EnrichedRecord, RecordScenery, RecordStatus,
    RowStatus, RowStatusSummary, SortDir, TableScenery, TableSceneryBuilder, ValueScenery,
    ValueSceneryBuilder, ValueStatus, boxed_custom_aggregate,
};
pub use servo::{Servo, ServoStatus};
pub use vantage_vista::VistaCapabilities;

/// Common imports for working with vantage-diorama.
///
/// ```
/// use vantage_diorama::prelude::*;
/// ```
pub mod prelude {
    pub use crate::augment::{Augmentation, Detail, Fetch, MergeRule, Source};
    pub use crate::dio::{Dio, DioEvent, Generation, WriteCapabilities};
    pub use crate::lens::{CacheBackend, CacheStatus, CacheTable, Lens, RedbCache};
    pub use crate::ops::{ChangeEvent, ChangeFlash, FlashKind};
    pub use crate::scenery::{
        EnrichedRecord, RecordScenery, RecordStatus, RowStatus, SortDir, TableScenery, ValueScenery,
    };
    pub use crate::servo::{Servo, ServoStatus};
    pub use vantage_vista_factory::VistaCatalog;
}
