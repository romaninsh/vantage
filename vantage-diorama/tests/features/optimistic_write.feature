Feature: Optimistic writes

  Step 4 — `dio.write_optimistic(op)` stages the new value in the cache and
  announces it (`WritePending`) before the write-through runs, so a form edit is
  visible instantly. On success the value is confirmed (`RecordChanged`); on
  failure the cache pre-image is restored and the error surfaced
  (`WriteReverted`) — the view reverts, never stuck on a value that didn't save.

  Scenario: an optimistic write commits — staged, then confirmed
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that records calls
    When the dio is created
    And I optimistically patch "a" with title "alpha"
    And I wait for 2 events
    Then the cache record "a" has title "alpha"
    And on_write has been called 1 time
    And the event log matches snapshot "optimistic_commit"

  Scenario: an optimistic write rolls back on failure — value reverts
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that always errors
    When the dio is created
    And I optimistically patch "a" with title "alpha"
    And I wait for 2 events
    Then the cache record "a" has title "one"
    And the event log matches snapshot "optimistic_rollback"
