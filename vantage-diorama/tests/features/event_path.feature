Feature: Event path

  Phase 4 — upstream `ChangeEvent` → `on_event` → cache mutation →
  internal `DioEvent` fanout, plus the Scenery-side generation-bump
  contract: every cache-affecting event reloads, `WriteFailed` does
  not.

  Scenario: ChangeEvent::Updated routes through on_event into the cache
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_event callback that calls dio.patched
    When the dio is created
    And a ChangeEvent::Updated arrives for id "a" with title "alpha"
    Then the cache record "a" has title "alpha"
    And the event log matches snapshot "patched_change_event"

  Scenario: notify_record_changed fires RecordChanged on the bus
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And dio.notify_record_changed is called for "a"
    Then the event log matches snapshot "notify_record_changed"

  Scenario: TableScenery generation advances on every cache-affecting event
    Given a master with rows
      | id | title |
      | a  | one   |
      | b  | two   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And the table scenery is opened
    Then the table scenery generation is 1
    When dio.notify_record_changed is called for "a"
    Then the table scenery generation is 2
    When dio.notify_dataset_changed is called
    Then the table scenery generation is 3

  Scenario: WriteFailed does not advance the TableScenery generation
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that always errors
    When the dio is created
    And the table scenery is opened
    Then the table scenery generation is 1
    When I insert via the facade
      | id | title |
      | b  | two   |
    Then the table scenery generation is 1
