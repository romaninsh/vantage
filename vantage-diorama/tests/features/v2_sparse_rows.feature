Feature: Sparse row access (v2)

  Phase v2-2 — once `row_count()` may exceed the cache size, `row(i)`
  returns `None` for indices outside the cached window. The Scenery
  publishes no event for these lookups; the UI renders a placeholder,
  the same way `app-csv-dio` already renders datetime cells while the
  background formatter is still resolving them. Crucially `row(i)` is
  a pure cache read — it does *not* trigger a master fetch on its own
  (that's `set_viewport`'s job in v2_viewport.feature).

  Scenario: row(i) returns Some for cached indices and None for the rest
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a paged loader that states the master count through set_total
    When the dio is created
    And the table scenery is opened
    Then the table scenery row at index 0 is Some
    And the table scenery row at index 99 is Some
    And the table scenery row at index 100 is None
    And the table scenery row at index 999 is None

  Scenario: row(i) does not trigger a master fetch on its own
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a paged loader that states the master count through set_total
    And an on_load_chunk callback that records calls
    When the dio is created
    And the table scenery is opened
    And the table scenery row at index 500 is queried 5 times
    Then the master has been listed 1 time, the harness seed included
    And on_load_chunk has been called 0 times

  Scenario: row(i) past estimated_total returns None
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a paged loader that states the master count through set_total
    When the dio is created
    And the table scenery is opened
    Then the table scenery row at index 1000 is None
    And the table scenery row at index 1500 is None

  Scenario: sparse cache survives a generation bump without dropping known rows
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a paged loader that states the master count through set_total
    When the dio is created
    And the table scenery is opened
    And dio.apply_change is called with Updated for the id at index 0 and no record
    And I wait for 1 event
    Then the table scenery row at index 0 is Some
    And the table scenery row at index 99 is Some
    And the table scenery row at index 100 is None
