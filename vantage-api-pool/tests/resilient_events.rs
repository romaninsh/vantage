//! `TransportEvent` variants that are `#[non_exhaustive]` can still be built
//! from outside the crate through their constructors.

use std::time::Duration;

use vantage_api_pool::resilient::{ClientError, ErrorKind, TransportEvent};

#[test]
fn constructors_build_events_with_the_given_fields() {
    match TransportEvent::succeeded(200, 12, Some(34)) {
        TransportEvent::Succeeded {
            status, ms, bytes, ..
        } => {
            assert_eq!((status, ms, bytes), (200, 12, Some(34)));
        }
        other => panic!("unexpected {other:?}"),
    }

    let error = ClientError::new(ErrorKind::Status(503), 2).with_body(Some("down".into()));
    match TransportEvent::failed(error.clone(), 7) {
        TransportEvent::Failed { error: got, ms, .. } => {
            assert_eq!((got, ms), (error, 7));
        }
        other => panic!("unexpected {other:?}"),
    }

    match TransportEvent::retry_scheduled(Duration::from_millis(50), 3) {
        TransportEvent::RetryScheduled { after, attempt, .. } => {
            assert_eq!((after, attempt), (Duration::from_millis(50), 3));
        }
        other => panic!("unexpected {other:?}"),
    }

    match TransportEvent::rows_pulled(9) {
        TransportEvent::RowsPulled { n, .. } => assert_eq!(n, 9),
        other => panic!("unexpected {other:?}"),
    }

    match TransportEvent::breaker_opened(Duration::from_secs(5)) {
        TransportEvent::BreakerOpened { cooldown, .. } => {
            assert_eq!(cooldown, Duration::from_secs(5));
        }
        other => panic!("unexpected {other:?}"),
    }
}
