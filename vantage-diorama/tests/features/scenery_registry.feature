Feature: Shared rows registry — owned views, shared rows, cancellation

  Step 2 — a UI widget binds to a Scenery; every open returns a view its
  caller owns, and views over the same query share ONE set of rows (one
  reactor, one cache window, one in-flight fetch) so opening is cheap.
  Releasing every handle aborts the background tasks — a closing grid stops
  pulling — and the registry entry self-heals, so nothing leaks.

  Scenario: opening the same query twice shares its rows
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that records calls and returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    When the dio is created
    And the table scenery is opened
    And the table scenery is opened again
    Then the two table sceneries are distinct views over the same rows
    And total_provider has been called 1 time
    And the dio has 2 live table sceneries

  Scenario: releasing every handle evicts the rows — no leak
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    When the dio is created
    And the table scenery is opened
    And the table scenery is opened again
    Then the dio has 2 live table sceneries
    When the table scenery handles are released
    Then the dio has 0 live table sceneries

  Scenario: a closing grid cancels its in-flight fetch
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    And the source has a read latency of 500 milliseconds
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 500..600
    And 100 milliseconds pass
    Then on_load_chunk has been called 1 time with range 500..600
    And the table scenery row at index 550 is None
    When the table scenery handles are released
    And 1000 milliseconds pass
    Then the event log contains no RangeLoaded
    And the dio has 0 live table sceneries
