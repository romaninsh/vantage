Feature: Write path — Delete

  v3 — facade `delete` enqueues a `Delete` `ChangeFlash`, staged in the
  cache first. Default-write routes to master, the Mirror helper writes
  the master, a confirmed delete keeps the staged removal, and an erroring
  `on_flash` reverts it (`WriteReverted`).

  Scenario: default_write routes Delete to master when on_flash is missing
    Given a master with rows
      | id | title |
      | a  | one   |
      | b  | two   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And I delete id "a" via the facade
    Then the master has 1 row
    And the master record "a" is absent
    And the cache has 1 row
    And the cache record "a" is absent

  Scenario: the delete is reverted on the bus when on_flash errors for Delete
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_flash route that always errors
    When the dio is created
    And I delete id "a" via the facade
    Then the event log matches snapshot "write_failed_delete"
    And the facade write failed with "on_flash rejected"
    And the cache has 1 row
    And the cache record "a" has title "one"

  Scenario Outline: Delete path works against real backends
    Given the backend is <backend>
    And a master with rows
      | id | title |
      | a  | one   |
      | b  | two   |
    And a lens with on_start that copies master to cache
    And an on_flash route that mirrors to master and cache
    When the dio is created
    And I delete id "a" via the facade
    And the write queue drains
    Then on_flash has been called 1 time
    And the event log matches snapshot "mirror_delete_<backend>"
    And the master has 1 row
    And the master record "a" is absent
    And the cache has 1 row
    And the cache record "a" is absent

    Examples:
      | backend |
      | mock    |
      | sqlite  |
