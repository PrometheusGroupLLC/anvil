Feature: Every read the engine serves a screen names the screen that asked

  The command line, the screens and the tool surface are peers of this engine.
  Two of them are already accountable in its log stream. The screens were not:
  a screen could ask this bridge for anything and leave no trace of having
  asked, so anything it then put in front of a person was state with no
  explanation behind it. That is a defect in the seam, not a mystery about the
  reader.

  So every read served over this bridge writes one seam record naming the
  command, the surface that asked, and how the turn came out. The record is
  closed labels and a method name. It carries no path, none of the caller's
  own input, and no identity — the same allowlist every other durable sink in
  this engine keeps. Its one prose field is a fixed label that is the same on
  every record, which is a name for the record and not a report about the turn.

  There is no acting person on this channel. It is loopback and read-only and
  carries no principal at all, so the record says the actor is unknown rather
  than leaving the field out. A missing actor reads as "nobody asked", which is
  a different claim from "this channel has no way to know".

  A request that does not name its surface is refused. It is not served under a
  default: a default is exactly how an unattributable read comes to look
  attributed, and a name the recorder supplied to itself records nothing.

  Scenario: A read names the screen that asked for it
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the engine is started with that hearth
    When a "playbook_atlas" request is sent over /ws naming the surface "playbooks-screen"
    Then the /ws JSON-RPC response carries a result
    And the engine stderr contains a JSON log record with fields:
      | seam    | ws_bridge        |
      | command | playbook_atlas   |
      | surface | playbooks-screen |
      | actor   | unknown          |
      | outcome | ok               |
    And the ws read seam record for "playbook_atlas" carries no "hearth" field
    And the ws read seam record for "playbook_atlas" carries no "hearth_path" field
    And the ws read seam record for "playbook_atlas" carries no "params" field

  Scenario: Two screens asking the same thing are told apart
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the engine is started with that hearth
    When a "playbook_atlas" request is sent over /ws naming the surface "playbooks-screen"
    And a "playbook_atlas" request is sent over /ws naming the surface "engineer-screen"
    Then the /ws JSON-RPC response carries a result
    And the engine stderr contains a JSON log record with fields:
      | seam    | ws_bridge        |
      | command | playbook_atlas   |
      | surface | playbooks-screen |
    And the engine stderr contains a JSON log record with fields:
      | seam    | ws_bridge        |
      | command | playbook_atlas   |
      | surface | engineer-screen  |

  Scenario: A read that does not name its surface is refused, not defaulted
    Given a playbook activity engine hearth with playbooks:
      | kind       | owner    | description       |
      | lore_query | lore-kit | Answer a question |
    And the engine is started with that hearth
    When a "playbook_atlas" request is sent over /ws naming no surface
    Then the /ws JSON-RPC response is an error envelope
    And the /ws JSON-RPC error.data.code is "surface_required"
    And the /ws JSON-RPC response carries no result
    And the engine stderr contains a JSON log record with fields:
      | seam    | ws_bridge                  |
      | command | playbook_atlas             |
      | surface | (unnamed)                  |
      | outcome | refused_surface_required   |
