@wip
Feature: Sequential load strategy — append-only paging (v4)

  Phase v4-1 — for masters that can only return "the next page" (cursor
  APIs, append-only logs, anything without a cheap COUNT and without
  random-access by row index). The Scenery has no `total_provider` and
  cannot honour a `set_viewport` for an uncached range. Instead it
  exposes a monotonic cache that grows page-by-page via
  `request_load_more`, and `row_count` / `estimated_total` grow with
  the cache.

  Background:
    Given a master with 1000 rows
    And a lens with on_load_next_page that returns up to 100 rows after the supplied index
    And no total_provider is configured
    When the dio is created
    And the table scenery is opened in sequential mode with page_size 100

  Scenario: opening seeds the first page
    Then on_load_next_page has been called 1 time with after=None limit=100
    And the cache contains 100 rows
    And the table scenery row_count is 100
    And the table scenery estimated_total is None
    And the table scenery has_more is true

  Scenario: request_load_more pages the next chunk after the cache end
    When request_load_more is called on the table scenery
    And I wait for 2 events
    Then on_load_next_page has been called 2 times
    And the last on_load_next_page after-index is 99
    And the cache contains 200 rows
    And the table scenery row_count is 200

  Scenario: a short final page flips has_more to false and freezes estimated_total
    Given the master has exactly 250 rows
    When request_load_more is called on the table scenery
    And request_load_more is called on the table scenery
    And I wait for 4 events
    Then on_load_next_page has been called 3 times
    And the cache contains 250 rows
    And the table scenery has_more is false
    And the table scenery estimated_total is Some(250)

  Scenario: set_viewport into the cached range is a no-op (no fetch)
    When the table scenery viewport is set to 10..50
    And I wait for 1 event
    Then on_load_next_page has been called 1 time

  Scenario: set_viewport past the cache end clamps to cached rows and emits ViewportClamped
    When the table scenery viewport is set to 500..600
    And I wait for 1 event
    Then on_load_next_page has been called 1 time
    And the event log contains ViewportClamped { requested: 500..600, clamped: 100..100 }

  Scenario: rapid repeated request_load_more coalesces into one in-flight page
    When request_load_more is called on the table scenery
    And request_load_more is called on the table scenery
    And request_load_more is called on the table scenery
    And I wait for 2 events
    Then on_load_next_page has been called 2 times
    And the cache contains 200 rows

  Scenario: failed on_load_next_page leaves the cache untouched and emits LoadFailed
    Given on_load_next_page errors on the second call
    When request_load_more is called on the table scenery
    And I wait for 2 events
    Then the cache contains 100 rows
    And the table scenery has_more is true
    And the event log contains LoadFailed { after: 99 }
