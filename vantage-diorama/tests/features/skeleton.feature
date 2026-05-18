Feature: Harness skeleton

  Phase-1 proof of life: the cucumber harness wires a master Vista
  through a Lens into a Dio, the on_start callback runs, and the cache
  ends up populated with the rows we declared.

  Scenario: mock backend builds, dio hydrates cache from master
    Given a master with rows
      | id | title |
      | a  | one   |
      | b  | two   |
    And a lens with on_start that copies master to cache
    When the dio is created
    Then the master responds to list with 2 rows
    And the cache contains 2 rows
