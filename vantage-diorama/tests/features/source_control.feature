Feature: Scriptable source — latency, counted faults, live mutation

  Step 1 — the test harness can model a slow / failing / mutating upstream
  deterministically under the paused clock (`tokio::time::advance`). The same
  knobs are the in-test stand-in for the real transport later phases build, so
  the diorama machinery is exercised against realistic source behaviour without
  any network.

  Scenario: a slow source still loads the viewport under virtual time
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    And the source has a read latency of 200 milliseconds
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 500..600
    And I wait for 2 events
    Then on_load_chunk has been called 1 time with range 500..600
    And the table scenery row at index 550 is Some
    And the event log matches snapshot "source_latency_loaded"

  Scenario: a counted source fault fails one load then recovers on refresh
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    And the source fails the next 1 read
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 500..600
    And I wait for 2 events
    Then the event log contains LoadFailed { range: 500..600 }
    And the table scenery row at index 550 is None
    When dio.refresh is called
    And I wait for 6 events
    Then the table scenery row at index 550 is Some

  Scenario: a mid-scenario source edit appears after refresh
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 200..205
    And I wait for 2 events
    Then the table scenery row at index 200 has title "row-200"
    When the source record "000200" title becomes "row-200-EDITED"
    And dio.refresh is called
    And I wait for 6 events
    Then the table scenery row at index 200 has title "row-200-EDITED"
