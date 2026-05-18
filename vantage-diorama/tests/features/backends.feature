@wip
Feature: Read paths across backends

  Phase 5 — same read scenario, three backends. CSV is read-only via
  the `vista` feature so it lives here, not in write_path.feature.
  The `Examples:` table is the only place a new backend is added.
  Drop the `@wip` tag once `MasterRows::build_csv` and
  `MasterRows::build_sqlite` are implemented in
  `tests/bdd_support/backend.rs`.

  Scenario Outline: facade list / get / count work uniformly
    Given the backend is <backend>
    And a master with rows
      | id | title |
      | a  | one   |
      | b  | two   |
    And a lens with on_start that copies master to cache
    When the dio is created
    Then the facade lists 2 rows
    And get_value "a" returns title "one"
    And the facade count is 2

    Examples:
      | backend |
      | mock    |
      | csv     |
      | sqlite  |
