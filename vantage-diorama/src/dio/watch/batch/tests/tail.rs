//! How a pass ends: with the stream, with nothing more ready, when the Dio
//! drops, or on a stream error.

use vantage_core::error;

use super::*;

/// Everything ready is applied in the pass; the stream's end is reported
/// after it.
#[tokio::test]
async fn a_ready_burst_is_one_pass_ending_with_the_stream() {
    let dio = dio().await;
    let mut stream = ready(vec![Ok(inserted("b", "1")), Ok(inserted("c", "1"))]);
    let (applied, tail) = pass(&dio, &mut stream).await;
    applied.unwrap();
    assert!(matches!(tail, Some(Tail::Ended)));
    assert_eq!(dio.inner.cache.count().await.unwrap(), 3);
}

#[tokio::test]
async fn a_stream_with_nothing_ready_applies_the_first_change() {
    let dio = dio().await;
    let mut stream: VistaChangeStream = Box::pin(futures::stream::pending());
    let (applied, tail) = pass(&dio, &mut stream).await;
    applied.unwrap();
    assert!(tail.is_none());
    assert_eq!(dio.inner.cache.count().await.unwrap(), 1);
}

/// A stream that never runs dry doesn't hold the Dio: once its last handle
/// drops, the pass ends.
#[tokio::test]
async fn a_never_dry_stream_ends_its_pass_when_the_dio_drops() {
    let dio = dio().await;
    let weak = Arc::downgrade(&dio.inner);
    let mut holder = Some(dio);
    let endless = futures::stream::iter(0..).map(move |i| {
        if i == 100 {
            drop(holder.take());
        }
        Ok(inserted(&i.to_string(), "1"))
    });
    let mut stream: VistaChangeStream = Box::pin(endless);
    let (_, tail) = apply_ready(&weak, inserted("a", "1"), &mut stream, &Admit::All).await;
    assert!(matches!(tail, Some(Tail::Dropped)));
}

#[tokio::test]
async fn a_stream_error_ends_the_pass_keeping_what_came_before() {
    let dio = dio().await;
    let mut stream = ready(vec![
        Ok(inserted("b", "1")),
        Err(error!("stream broke")),
        Ok(inserted("c", "1")),
    ]);
    let (applied, tail) = pass(&dio, &mut stream).await;
    applied.unwrap();
    assert!(matches!(tail, Some(Tail::Failed(_))));
    let cache = &dio.inner.cache;
    assert_eq!(cache.count().await.unwrap(), 2);
    assert_eq!(cache.get_value("c").await.unwrap(), None);
}
