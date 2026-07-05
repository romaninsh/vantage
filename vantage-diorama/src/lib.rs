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
    MemoryCacheTable,
};
pub use ops::{ChangeEvent, QueryDescriptor, WriteOp};
pub use scenery::{
    Aggregate, CustomAggregate, EnrichedRecord, RecordScenery, RecordStatus, RowStatus,
    RowStatusSummary, SortDir, TableScenery, TableSceneryBuilder, ValueScenery,
    ValueSceneryBuilder, ValueStatus, boxed_custom_aggregate,
};
pub use vantage_vista::VistaCapabilities;
