Feature: Refresh ticker

  Phase 5 — `refresh_every` interval semantics under virtual time
  (`tokio::time::pause` / `advance`). The critical invariant: the
  first tick is skipped so the refresh fires after the interval,
  not at t=0.

  Scenario: refresh_every does not fire at t=0
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And a refresh interval of 60 seconds
    And an on_refresh callback that records calls
    When the dio is created
    Then on_refresh has been called 0 times
    When 59 seconds pass
    Then on_refresh has been called 0 times
    When 2 seconds pass
    And I wait for 2 events
    Then on_refresh has been called 1 time
    And the event log matches snapshot "refresh_skip_first"

  Scenario: manual dio.refresh fires on_refresh and publishes Invalidated
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_refresh callback that records calls
    When the dio is created
    And dio.refresh is called
    And I wait for 2 events
    Then on_refresh has been called 1 time
    And the event log matches snapshot "manual_refresh"

  # A refresh tick whose on_refresh fails (e.g. the server 503s) must leave the
  # painted grid alone: it may announce Refreshing, but it must NOT publish
  # Invalidated, because Invalidated makes sceneries reseed from cache — which
  # reverts the grid to a stale snapshot and drops rows added since. This
  # mirrors the guarantee manual `dio.refresh()` already gives.
  Scenario: a failed auto-refresh does not publish Invalidated
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And a refresh interval of 60 seconds
    And an on_refresh callback that fails
    When the dio is created
    And 61 seconds pass
    And I wait for 1 event
    Then on_refresh has been called 1 time
    And the event log contains no Invalidated
