Feature: Viewport-driven chunk loading (v2)

  Phase v2-3 — the UI declares its visible range via
  `set_viewport(range)`; the Scenery debounces those calls and asks
  the configured `on_load_chunk` closure to fetch the missing indices
  from the master. New rows land in the cache and a `RangeLoaded`
  event fans out, distinct from the existing whole-scenery
  `DatasetChanged`. `request_load_more` is the explicit "page the next
  chunk" trigger for callers that don't want to drive loading from
  scroll position alone.

  Scenario: set_viewport into an uncached range triggers a chunk load
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 500..600
    And I wait for 2 events
    Then on_load_chunk has been called 1 time with range 500..600
    And the cache contains 200 rows
    And the table scenery row at index 550 is Some
    And the event log matches snapshot "viewport_range_loaded"

  Scenario: set_viewport inside the cached range does not refetch
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that records calls
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 10..50
    And I wait for 1 event
    Then on_load_chunk has been called 0 times

  Scenario: rapid viewport changes coalesce into a single load
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 300..400
    And the table scenery viewport is set to 400..500
    And the table scenery viewport is set to 500..600
    And I wait for 2 events
    Then on_load_chunk has been called 1 time
    And the last on_load_chunk range is 500..600

  Scenario: request_load_more pages the next chunk past the current cache end
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    When the dio is created
    And the table scenery is opened
    And request_load_more is called on the table scenery
    And I wait for 2 events
    Then on_load_chunk has been called 1 time with range 100..200
    And the cache contains 200 rows
    And the table scenery generation is 2

  Scenario: RangeLoaded carries the loaded range and does not invalidate the whole scenery
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that pulls the requested range from master
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 500..600
    And I wait for 2 events
    Then the table scenery generation is 2

  Scenario: failed on_load_chunk leaves the cache untouched and emits LoadFailed
    Given a master with 1000 rows
    And a lens with on_start that copies the first 100 rows to cache
    And a total_provider that returns the master count
    And an on_load_chunk callback that always errors
    When the dio is created
    And the table scenery is opened
    And the table scenery viewport is set to 500..600
    And I wait for 2 events
    Then the cache contains 100 rows
    And the table scenery generation is 1
    And the event log matches snapshot "load_chunk_failed"
