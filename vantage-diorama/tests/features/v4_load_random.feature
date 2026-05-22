Feature: Random-access load strategy — viewport-driven sparse paging (v4)

  Phase v4-2 — locks in the random-access path. With `total_provider`
  + `on_load_chunk`, the Scenery is in random mode: the scrollbar
  sizes to the master total before any rows are loaded, and
  `set_viewport` drives all fetching. The master Vista advertises
  `can_fetch_page: true`, and UI delegates branch on that flag —
  random-access masters skip `request_load_more` entirely, so
  jumping the scrollbar past the cached region fetches the visible
  band directly instead of marching from index 0.

  Background:
    Given a master with 1000 rows
    And a lens with on_load_chunk that pulls the requested range from master
    And a total_provider that returns the master count
    When the dio is created
    And the table scenery is opened in random mode with page_size 100

  Scenario: opening with refresh_on_open seeds the first page only
    Then on_load_chunk has been called 1 time with range 0..100
    And the cache contains 100 rows
    And the table scenery row_count is 1000
    And the table scenery estimated_total is Some(1000)
    And the table scenery has_more is true

  Scenario: master capabilities advertise random-access paging
    Then the table scenery master capability can_fetch_page is true

  Scenario: set_viewport for a fully uncached range fetches a page-aligned chunk
    When the table scenery viewport is set to 547..567
    And I wait for 4 events
    Then on_load_chunk has been called 2 times
    And the last on_load_chunk range is 547..647

  Scenario: set_viewport with overlap re-fetches only the missing tail
    Given the cache already contains rows 547..647
    When the table scenery viewport is set to 620..640
    And I wait for 1 event
    Then on_load_chunk has been called 1 time
    # 620..640 already in the 547..647 seed; compute_fetch_range returns
    # None and the seed's call (rewound by the Given) is the only one
    # counted is the refresh_on_open call from Background.

  Scenario: dragging to the end fetches the visible range, not a march from cache top
    Given the cache already contains rows 0..200
    When the table scenery viewport is set to 980..1000
    And I wait for 4 events
    Then on_load_chunk has been called 2 times
    And the last on_load_chunk range is 980..1000

  Scenario: scrolling backwards into an uncached region fills the gap, not the cache edge
    Given the cache already contains rows 800..900
    When the table scenery viewport is set to 750..770
    And I wait for 4 events
    Then on_load_chunk has been called 2 times
    And the last on_load_chunk range is 750..850

  Scenario: rapid viewport bursts coalesce; stale viewports are dropped
    When the table scenery viewport is set to 300..320
    And the table scenery viewport is set to 400..420
    And the table scenery viewport is set to 500..520
    And I wait for 4 events
    Then on_load_chunk has been called 2 times
    And the last on_load_chunk range is 500..600

  Scenario: set_viewport past total clamps to total
    When the table scenery viewport is set to 1900..2000
    And I wait for 3 events
    Then on_load_chunk has been called 1 time

  Scenario: failed on_load_chunk leaves the cache untouched and emits LoadFailed
    Given on_load_chunk errors on the next call
    When the table scenery viewport is set to 500..600
    And I wait for 4 events
    Then the cache contains 100 rows
    And the event log contains LoadFailed { range: 500..600 }
