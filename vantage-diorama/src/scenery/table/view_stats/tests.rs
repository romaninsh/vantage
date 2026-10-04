use super::*;

fn stats(total: Option<usize>, total_exact: bool, loaded: usize, has_more: bool) -> ViewStats {
    ViewStats {
        total,
        total_exact,
        loaded,
        has_more,
        ..Default::default()
    }
}

#[test]
fn count_exact_total() {
    let s = stats(Some(1240), true, 300, true);
    assert_eq!(s.count(), ViewCount::Exact(1240));
    assert_eq!(s.count_text(), "1,240");
}

#[test]
fn count_estimated_total() {
    let s = stats(Some(1200), false, 300, true);
    assert_eq!(s.count(), ViewCount::Estimated(1200));
    assert_eq!(s.count_text(), "~1,200");
}

#[test]
fn count_no_total_with_more_is_a_lower_bound() {
    let s = stats(None, false, 300, true);
    assert_eq!(s.count(), ViewCount::AtLeast(300));
    assert_eq!(s.count_text(), "300+");
}

#[test]
fn count_no_total_nothing_more_is_loaded() {
    let s = stats(None, false, 42, false);
    assert_eq!(s.count(), ViewCount::Loaded(42));
    assert_eq!(s.count_text(), "42");
    assert_eq!(s.count().value(), 42);
}

#[test]
fn capped_at_cap_once_cap_rows_are_loaded() {
    let s = ViewStats {
        showing: Some(5..40),
        ..stats(None, false, 120, true)
    }
    .capped(25);
    assert_eq!((s.total, s.total_exact, s.loaded), (Some(25), true, 25));
    assert!(!s.has_more);
    assert_eq!(s.showing, Some(5..25));
    assert_eq!(s.showing_len(), Some(20));
    assert_eq!(s.count_text(), "25");
}

#[test]
fn capped_below_cap_keeps_the_inner_answer() {
    let estimate = stats(Some(1200), false, 10, true).capped(50);
    assert_eq!(estimate.count(), ViewCount::Estimated(50));
    assert!(estimate.has_more);

    let small = stats(Some(8), true, 8, false).capped(50);
    assert_eq!(small.count(), ViewCount::Exact(8));
}

#[test]
fn capped_drops_a_viewport_past_the_cap() {
    let s = ViewStats {
        showing: Some(60..80),
        ..stats(Some(100), true, 100, false)
    }
    .capped(50);
    assert_eq!(s.showing, None);
    assert_eq!(s.showing_count(), 0);
    assert_eq!(s.showing_len(), None);
}

#[test]
fn state_marker_precedence() {
    let rows = |pending_write, failed| RowStatusSummary {
        pending_write,
        failed,
        ..Default::default()
    };
    use LoadState::*;
    assert_eq!(
        ViewState::derive(Loading, &rows(1, 1), true),
        ViewState::Loading
    );
    assert_eq!(
        ViewState::derive(Partial, &rows(1, 1), true),
        ViewState::Failed
    );
    assert_eq!(
        ViewState::derive(Partial, &rows(1, 0), true),
        ViewState::PendingWrites
    );
    assert_eq!(
        ViewState::derive(Complete, &rows(0, 0), true),
        ViewState::Stale
    );
    assert_eq!(
        ViewState::derive(Complete, &rows(0, 0), false),
        ViewState::Ready
    );
}
