Feature: Reference traversal

  Traversing a reference returns a new Dio bound to the related table. Because
  that Dio loads through the normal cache-first, failure-tolerant path, a
  reference whose source is temporarily unreachable must never read as if the
  reference does not exist. The only legitimate traversal failure is a reference
  that genuinely isn't defined.

  Background:
    Given a launch master with a crew reference behind a warm-cache lens

  Scenario: traversal resolves and lists the related rows
    When the "crew" reference is traversed from launch "L1"
    Then the traversed dio lists 1 crew member

  Scenario: an undefined reference is the only legitimate failure
    Then traversing "engines" from launch "L1" fails

  Scenario: a dead source does not read as a missing reference
    When the "crew" reference is traversed from launch "L1"
    Then the traversed dio lists 1 crew member
    When the crew source goes offline
    And the "crew" reference is traversed from launch "L1"
    Then the traversed dio lists 1 crew member

  Scenario: each launch gets its own crew cache
    Then crew caches for launches "L1" and "L2" are distinct
