Feature: Playbook access granted scoping

  BP2/BP3 (workflow_access_scoping): access context must round-trip through the
  domain request types, absent context must fall back to the default-safe
  Foundation/read/internal/no-space context, and driven candidates must be
  filtered by the GRANTED predicate before routing or begin selection.

  Scenario: explicit ctx round-trips into the domain route request
    Given a request context with org "acme" role "write" clearance "confidential" space "team-1"
    Then the domain route request ctx is org "acme" role "write" clearance "confidential" space "team-1"
    And the domain begin request ctx is org "acme" role "write" clearance "confidential" space "team-1"

  Scenario: absent ctx yields the default-safe context
    Given domain route and begin requests are built without ctx
    Then the domain route request ctx is org "Foundation" role "read" clearance "internal" space ""
    And the domain begin request ctx is org "Foundation" role "read" clearance "internal" space ""

  Scenario: org scoping grants wildcard and matching org but denies a non-matching org
    Given a driven access registry with machines:
      | kind             | org        | min_role | sensitivity | space |
      | org_wildcard     | Foundation | read     | internal    | none  |
      | org_match        | acme       | read     | internal    | none  |
      | org_mismatch     | globex     | read     | internal    | none  |
    And a request context with org "acme" role "admin" clearance "phi" space "team-1"
    When granted driven candidates are selected
    Then the granted kinds include kind "org_wildcard"
    And the granted kinds include kind "org_match"
    And the granted kinds exclude kind "org_mismatch"

  Scenario: role scoping denies below-minimum role and grants equal or above roles
    Given a driven access registry with machines:
      | kind       | org  | min_role | sensitivity | space |
      | role_below | acme | admin    | internal    | none  |
      | role_equal | acme | write    | internal    | none  |
      | role_above | acme | read     | internal    | none  |
    And a request context with org "acme" role "write" clearance "phi" space "team-1"
    When granted driven candidates are selected
    Then the granted kinds exclude kind "role_below"
    And the granted kinds include kind "role_equal"
    And the granted kinds include kind "role_above"

  Scenario: sensitivity scoping denies below-clearance requests and grants equal or above clearance
    Given a driven access registry with machines:
      | kind              | org  | min_role | sensitivity  | space |
      | sensitivity_below | acme | read     | phi          | none  |
      | sensitivity_equal | acme | read     | confidential | none  |
      | sensitivity_above | acme | read     | internal     | none  |
    And a request context with org "acme" role "admin" clearance "confidential" space "team-1"
    When granted driven candidates are selected
    Then the granted kinds exclude kind "sensitivity_below"
    And the granted kinds include kind "sensitivity_equal"
    And the granted kinds include kind "sensitivity_above"

  Scenario: space scoping grants no-space and matching-space machines but denies mismatches
    Given a driven access registry with machines:
      | kind           | org  | min_role | sensitivity | space  |
      | space_wildcard | acme | read     | internal    | none   |
      | space_match    | acme | read     | internal    | team-1 |
      | space_mismatch | acme | read     | internal    | team-2 |
    And a request context with org "acme" role "admin" clearance "phi" space "team-1"
    When granted driven candidates are selected
    Then the granted kinds include kind "space_wildcard"
    And the granted kinds include kind "space_match"
    And the granted kinds exclude kind "space_mismatch"

  Scenario: default-safe context only grants baseline access and denies restricted machines
    Given a driven access registry with machines:
      | kind                  | org        | min_role | sensitivity | space  |
      | foundation_baseline   | Foundation | read     | internal    | none   |
      | restricted_all_axes   | acme       | admin    | phi         | team-1 |
    And a request context with org "Foundation" role "read" clearance "internal" space ""
    When granted driven candidates are selected
    Then the granted kinds include kind "foundation_baseline"
    And the granted kinds exclude kind "restricted_all_axes"
