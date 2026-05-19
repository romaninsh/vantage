Feature: Decoupled total-row count (v2)

  Phase v2-1 — `TableScenery` separates "rows currently in cache" from
  "rows declared to exist". A `total_provider` closure queries the
  master cheaply (e.g. `SELECT COUNT(*)` on sqlite) so a UI can size
  its scrollbar before any rows are paged into the cache. Missing the
  provider keeps v1 behaviour bit-identical: counts derive from cache
  size, as they do today.

  Scenario: estimated_total comes from the total_provider, not the cache
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    When the dio is created
    And the table scenery is opened
    Then the cache contains 100 rows
    And the table scenery row_count is 1000
    And the table scenery estimated_total is 1000

  Scenario: missing total_provider falls back to cache size (v1 behaviour)
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And no total_provider is configured
    When the dio is created
    And the table scenery is opened
    Then the cache contains 100 rows
    And the table scenery row_count is 100
    And the table scenery estimated_total is 100

  Scenario: total_provider runs once per scenery open, not per row read
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that records calls and returns the master count
    When the dio is created
    And the table scenery is opened
    And the table scenery row at index 0 is queried 50 times
    Then total_provider has been called 1 time
