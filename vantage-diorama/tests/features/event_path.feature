Feature: Event path

  Phase 4 — upstream `ChangeEvent` → `apply_change` → cache mutation →
  internal `DioEvent` fanout, plus the Scenery-side generation-bump
  contract: every cache-affecting event reloads, `WriteFailed` does
  not.

  Scenario: ChangeEvent::Updated routes through apply_change into the cache
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And a ChangeEvent::Updated arrives for id "a" with title "alpha"
    Then the cache record "a" has title "alpha"
    And the event log matches snapshot "patched_change_event"

  Scenario: an Updated change with no record fires RecordChanged on the bus
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And dio.apply_change is called with Updated for "a" and no record
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
    When dio.apply_change is called with Updated for "a" and no record
    Then the table scenery generation is 2
    When dio.reconcile is called with the cached rows
    Then the table scenery generation is 3

  Scenario: WriteFailed does not advance the TableScenery generation
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_flash route that always errors
    When the dio is created
    And the table scenery is opened
    Then the table scenery generation is 1
    When I delete all via the facade
    Then the event log contains WriteFailed
    And the table scenery generation is 1
