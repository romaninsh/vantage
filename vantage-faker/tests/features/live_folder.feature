Feature: Live folder simulation
  A LiveFolderSim models a constantly-mutating multi-layer log tree.

  Scenario: Backfill populates the tree
    Given a live-folder sim with a 1-hour backfill
    Then the tree contains at least one date folder
    And the tree contains an access_logs_HH folder for today
    And at least one chunk file exists under an access_logs folder

  Scenario: A first-call listing vista seeds from the current tree
    Given a live-folder sim with a 1-hour backfill
    When I open the listing vista for the root path
    Then the listing contains at least one row

  Scenario: A second listing call on the same path shares the store
    Given a live-folder sim with a 1-hour backfill
    When I open the listing vista for the root path twice
    Then both vistas report the same number of rows

  Scenario: Folder size vista returns none for an unknown path
    Given a live-folder sim with no backfill
    When I fetch the size for path does-not-exist
    Then the result is none

  Scenario: Folder size vista returns size for a real folder
    Given a live-folder sim with a 2-minute backfill
    When I fetch the size for any populated folder
    Then the size record has both size and file_count

  Scenario: Error files appear with errors-log suffix
    Given a live-folder sim with a 1-hour backfill and a 100-percent error rate
    Then at least one error log file exists under an error_logs folder

  Scenario: Event files appear with log suffix
    Given a live-folder sim with a 1-hour backfill
    Then at least one event file exists under an events folder
    And every event file name is one of the declared event types

  Scenario: Chunk rolls when threshold is met
    Given a live-folder sim with no backfill and a 200-byte chunk threshold
    When I simulate 5 virtual seconds at 100 bytes per second
    Then at least 2 chunk files exist under an access_logs folder
