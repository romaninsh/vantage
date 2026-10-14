Feature: Decoupled total-row count (v2)

  Phase v2-1 — `TableScenery` separates "rows currently in cache" from
  "rows declared to exist". A paged loader states the master's count
  through `set_total` (e.g. `SELECT COUNT(*)` on sqlite), and the cache
  remembers it, so a UI can size its scrollbar before any rows are paged
  into the cache. Without a stated total, counts derive from the cache
  size.

  Scenario: estimated_total comes from the remembered total, not the cache size
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a paged loader that states the master count through set_total
    When the dio is created
    And the table scenery is opened
    Then the cache contains 100 rows
    And the table scenery row_count is 1000
    And the table scenery estimated_total is 1000

  Scenario: without a stated total, estimated_total falls back to the cache size
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And no loader states a total
    When the dio is created
    And the table scenery is opened
    Then the cache contains 100 rows
    And the table scenery row_count is 100
    And the table scenery estimated_total is 100

  Scenario: the harness seed states the total once; row reads don't restate it
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a paged loader that states the master count through set_total and records each count
    When the dio is created
    And the table scenery is opened
    And the table scenery row at index 0 is queried 50 times
    Then the master total has been stated 1 time, the harness seed included
