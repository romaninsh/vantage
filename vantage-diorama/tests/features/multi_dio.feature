Feature: Multiple Dios under one Lens

  Phase 5 — a single Lens may produce many Dios, each binding a
  different master Vista. They share the cache backend but claim
  distinct cache tables (named after the master).

  Scenario: two dios share the cache backend but isolate by table name
    Given a master named "products" with rows
      | id | title |
      | a  | one   |
    And a master named "orders" with rows
      | id | label |
      | x  | foo   |
    And a lens with on_start that copies master to cache
    When the dio for "products" is created
    And the dio for "orders" is created
    Then the "products" cache contains 1 row
    And the "orders" cache contains 1 row
    And the two cache tables are distinct
