Feature: Write path — Patch

  v3 — facade `patch_value` enqueues `WriteOp::Patch`, which the
  default-write path forwards to master and `on_write` can intercept.
  Patch merges: columns absent from the partial must survive on the
  authoritative record, both for the default-route master path and for
  the Mirror helper's cache write.

  Scenario: default_write routes Patch to master when on_write is missing
    Given a master with rows
      | id | title | price |
      | a  | one   | 10    |
    And a lens with on_start that copies master to cache
    When the dio is created
    And I patch via the facade
      | id | title |
      | a  | alpha |
    Then the master record "a" has title "alpha"
    And the master record "a" has price "10"
    And the cache record "a" has title "one"
    And the cache record "a" has price "10"

  Scenario: WriteFailed lands on the bus when on_write errors for Patch
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    And an on_write callback that always errors
    When the dio is created
    And I patch via the facade
      | id | title |
      | a  | alpha |
    Then the event log matches snapshot "write_failed_patch"

  Scenario Outline: Patch path works against real backends
    Given the backend is <backend>
    And a master with rows
      | id | title | price |
      | a  | one   | 10    |
    And a lens with on_start that copies master to cache
    And an on_write callback that mirrors to master and cache
    When the dio is created
    And I patch via the facade
      | id | title |
      | a  | alpha |
    And the write queue drains
    Then on_write has been called 1 time
    And the event log matches snapshot "mirror_patch_<backend>"
    And the master record "a" has title "alpha"
    And the master record "a" has price "10"
    And the cache record "a" has title "alpha"
    And the cache record "a" has price "10"

    Examples:
      | backend |
      | mock    |
      | sqlite  |
