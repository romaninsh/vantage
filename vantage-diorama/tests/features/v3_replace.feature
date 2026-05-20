Feature: Write path — Replace

  v3 — facade `replace_value` enqueues `WriteOp::Replace`, which the
  default-write path forwards to master and `on_write` can intercept.
  Mirrors the Insert trio in `write_path.feature` for the replace op:
  default-route, WriteFailed, and the mock+sqlite mirror outline.

  Scenario: default_write routes Replace to master when on_write is missing
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And I replace via the facade
      | id | title |
      | a  | alpha |
    Then the master has 1 row
    And the master record "a" has title "alpha"
    And the cache record "a" has title "one"

  Scenario: WriteFailed lands on the bus when on_write errors for Replace
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that always errors
    When the dio is created
    And I replace via the facade
      | id | title |
      | a  | alpha |
    Then the event log matches snapshot "write_failed_replace"

  Scenario Outline: Replace path works against real backends
    Given the backend is <backend>
    And a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that mirrors to master and cache
    When the dio is created
    And I replace via the facade
      | id | title |
      | a  | alpha |
    And the write queue drains
    Then on_write has been called 1 time
    And the event log matches snapshot "mirror_replace_<backend>"
    And the master record "a" has title "alpha"
    And the cache record "a" has title "alpha"

    Examples:
      | backend |
      | mock    |
      | sqlite  |
