//! DioRouter end-to-end over an in-process Dio: plain GETs serve the cache
//! without fetching, detail GETs hydrate once, watches stream ADDED then
//! MODIFIED lines as augmentation lands, and a closed watch connection
//! releases its scenery.
#![cfg(feature = "axum")]

mod fixture;
mod reads;
mod watches;
