Feature: Write path — DeleteAll

  v3 — facade `delete_all` enqueues `WriteOp::DeleteAll`. Default-write
  routes to master (clearing it), the Mirror helper clears both, and an
  erroring `on_write` publishes `WriteFailed`.

  Scenario: default_write routes DeleteAll to master when on_write is missing
    Given a master with rows
      | id | title |
      | a  | one   |
      | b  | two   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And I delete all via the facade
    Then the master has 0 rows
    And the cache still has 2 rows

  Scenario: WriteFailed lands on the bus when on_write errors for DeleteAll
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that always errors
    When the dio is created
    And I delete all via the facade
    Then the event log matches snapshot "write_failed_delete_all"

  Scenario Outline: DeleteAll path works against real backends
    Given the backend is <backend>
    And a master with rows
      | id | title |
      | a  | one   |
      | b  | two   |
    And a lens with on_start that copies master to cache
    And an on_write callback that mirrors to master and cache
    When the dio is created
    And I delete all via the facade
    And the write queue drains
    Then on_write has been called 1 time
    And the event log matches snapshot "mirror_delete_all_<backend>"
    And the master has 0 rows
    And the cache has 0 rows

    Examples:
      | backend |
      | mock    |
      | sqlite  |
