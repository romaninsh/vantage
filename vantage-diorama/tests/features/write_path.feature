Feature: Write path

  Phase 3 — `on_write` callback wiring, capability lifting on the
  facade Vista, and the `WriteFailed` event contract when an `on_write`
  closure returns an error.

  Scenario: on_write lifts can_insert/can_update/can_delete on the facade
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that records calls
    When the dio is created
    Then the facade capability can_insert is true
    And the facade capability can_update is true
    And the facade capability can_delete is true

  Scenario: default_write routes to master when on_write is missing
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And I insert via the facade
      | id | title |
      | b  | two   |
    Then the master has 2 rows
    And the cache still has 1 row

  Scenario: WriteFailed lands on the bus when on_write errors
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that always errors
    When the dio is created
    And I insert via the facade
      | id | title |
      | b  | two   |
    Then the event log matches snapshot "write_failed"

  @wip
  Scenario Outline: write path works against real backends
    Given the backend is <backend>
    And a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that mirrors to master and cache
    When the dio is created
    And I insert via the facade
      | id | title |
      | b  | two   |
    Then the cache has 2 rows
    And the master has 2 rows

    Examples:
      | backend |
      | mock    |
      | sqlite  |
