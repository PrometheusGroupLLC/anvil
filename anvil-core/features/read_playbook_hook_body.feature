Feature: Read playbook hook body via QueryPort
  The QueryPort exposes a read_playbook_hook_body operation that retrieves
  the content of a named hook file belonging to a specific playbook. This
  provides the body-read seam that begin will use in P3/P4 to serve
  hook content into context channels without hardcoding file paths.

  Scenario: reading a seeded hook body returns the content verbatim
    Given an in-memory query adapter seeded with hook body for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md" content "HOOK_BODY_DISTINCTIVE_CONTENT_XYZ"
    When query_port.read_playbook_hook_body is called for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md"
    Then the query port returns hook body containing "HOOK_BODY_DISTINCTIVE_CONTENT_XYZ"

  Scenario: reading a missing playbook hook body returns an IoError
    Given an in-memory query adapter with no artifacts seeded
    When query_port.read_playbook_hook_body is called for playbook "20260422T0000_track_lifecycle" filename "spec-writing.md"
    Then the query port returns a hook body IoError containing "spec-writing.md"
