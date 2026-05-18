Feature: Lens lifecycle

  The three contracts that make a Lens behave predictably: on_start
  blocks make_dio iff `on_start_blocking` is true, and the per-Dio
  write worker shuts down cleanly when the last external Dio handle
  drops.

  Scenario: on_start_blocking=true makes make_dio wait for the callback
    Given a master with rows
      | id | title |
      | a  | one   |
    And a gated on_start that copies master to cache
    And on_start_blocking is true
    When I spawn make_dio
    Then make_dio is still pending
    And on_start has been called 0 times
    When I release on_start
    Then make_dio completes
    And on_start has been called 1 time

  Scenario: on_start_blocking=false lets make_dio return immediately
    Given a master with rows
      | id | title |
      | a  | one   |
    And a gated on_start that copies master to cache
    And on_start_blocking is false
    When the dio is created
    Then on_start has been called 0 times
    When I release on_start
    Then on_start has been called 1 time

  Scenario: dropping the last Dio handle stops the write worker cleanly
    Given a master with rows
      | id | title |
      | a  | one   |
    And a lens with on_start that copies master to cache
    When the dio is created
    And I capture the write worker handle
    And I drop the dio
    Then the write worker exits cleanly
