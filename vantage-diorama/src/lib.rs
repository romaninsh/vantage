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

pub use augment::{
    AugmentSpec, Augmentation, BuildFn, Detail, Fetch, FetchFn, FetchSpec, MergeRule, SetOp,
    Source, SourceSpec, lower_augment,
};
pub use composition::Diorama;
pub use dio::diagnostics::{DioDiagnostics, SceneryDiagnostic};
pub use dio::{Dio, DioEvent, DioShell, Generation};
pub use error::{DioError, LensBuildError};
pub use lens::{
    Activity, ActivitySignal, CacheBackend, CacheStatus, CacheTable, ChunkRow, ChunkSink,
    DioCallback, DioEventCallback, DioLoadChunkCallback, DioTotalProviderCallback,
    DioWriteCallback, Lens, LensBuilder, LensCallbacks, LensDefaults, MemoryCache,
    MemoryCacheTable, RedbCache, RedbCacheTable,
};
pub use ops::{ChangeEvent, QueryDescriptor, WriteOp};
pub use scenery::{
    Aggregate, CappedScenery, CustomAggregate, EnrichedRecord, RecordScenery, RecordStatus,
    RowStatus, RowStatusSummary, SortDir, TableScenery, TableSceneryBuilder, ValueScenery,
    ValueSceneryBuilder, ValueStatus, boxed_custom_aggregate,
};
pub use vantage_vista::VistaCapabilities;

/// Common imports for working with vantage-diorama.
///
/// ```
/// use vantage_diorama::prelude::*;
/// ```
pub mod prelude {
    pub use crate::augment::{Augmentation, Detail, Fetch, MergeRule, Source};
    pub use crate::dio::{Dio, DioEvent, Generation};
    pub use crate::lens::{CacheBackend, CacheStatus, CacheTable, Lens, RedbCache};
    pub use crate::ops::{ChangeEvent, WriteOp};
    pub use crate::scenery::{
        EnrichedRecord, RecordScenery, RecordStatus, RowStatus, SortDir, TableScenery, ValueScenery,
    };
    pub use vantage_vista_factory::VistaCatalog;
}
