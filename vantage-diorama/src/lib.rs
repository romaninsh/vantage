#![doc = include_str!("../README.md")]
// Stage 1 is shape-only — fields/methods land their first callers in stages 2+.
#![allow(dead_code)]

pub mod composition;
pub mod dio;
pub mod error;
pub mod lens;
pub mod ops;
pub mod scenery;

pub use composition::Diorama;
pub use dio::{Dio, DioEvent, DioShell, Generation};
pub use error::{DioError, LensBuildError};
pub use lens::{
    CacheBackend, DioCallback, DioEventCallback, DioQueryCallback, DioWriteCallback, Lens,
    LensBuilder, LensCallbacks, LensDefaults,
};
pub use ops::{ChangeEvent, QueryDescriptor, WriteOp};
pub use scenery::{
    Aggregate, CustomAggregate, EnrichedRecord, RecordScenery, RecordStatus, RowStatus, SortDir,
    TableScenery, TableSceneryBuilder, ValueScenery, ValueSceneryBuilder, ValueStatus,
    boxed_custom_aggregate,
};
