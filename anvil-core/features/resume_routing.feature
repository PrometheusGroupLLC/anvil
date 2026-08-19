Feature: Resume-aware routing — continuation tokens and open-playbook lookup
  The pure resume seam bridges a continuation message to the conversation's open
  playbook. `is_continuation_token` recognizes only the fixed token set
  (case-insensitive, trimmed); `find_open_playbook_run_for_conversation` selects the
  conversation's open (begun, non-terminal) playbook, returning the
  most-recently-begun on multiplicity and never a terminal artifact. The track
  seed treats `completed` as NON-terminal (it can enter the amend loop), so a
  truly closed track is `abandoned`/`superseded` — that is what the terminal
  exclusion (plan H2, keyed on StateDefinition.is_terminal) filters out.
  (resume_aware_routing Phases 2 & 3, spec req #2/#3.)

  Scenario Outline: the continuation predicate recognizes the fixed token set
    When the continuation predicate is evaluated for "<message>"
    Then the evaluated message is a continuation token

    Examples:
      | message     |
      | go          |
      | continue    |
      | proceed     |
      | next        |
      | yes         |
      | ok          |
      | keep going  |
      | GO          |
      |   go        |
      | Keep Going  |

  Scenario Outline: non-tokens are not continuation signals
    When the continuation predicate is evaluated for "<message>"
    Then the evaluated message is not a continuation token

    Examples:
      | message                       |
      | implement the resume feature  |
      | go fish                       |
      |                               |
      | gogo                          |
      | yesterday                     |

  Scenario: an open non-terminal playbook for the conversation is found
    Given a resume adapter with track "20260601T0001_alpha" kind "track" state "spec" conversation_id "C1" begun at "2026-06-01T10:00:00Z"
    When the open playbook is looked up for conversation_id "C1"
    Then the resume lookup returns artifact "20260601T0001_alpha" kind "track" state "spec"

  Scenario: a different conversation's open playbook is not returned
    Given a resume adapter with track "20260601T0001_alpha" kind "track" state "spec" conversation_id "C1" begun at "2026-06-01T10:00:00Z"
    When the open playbook is looked up for conversation_id "C2"
    Then the resume lookup returns no open playbook

  Scenario: a terminal (abandoned) playbook is not treated as open
    Given a resume adapter with track "20260601T0002_done" kind "track" state "abandoned" conversation_id "C3" begun at "2026-06-01T10:00:00Z"
    When the open playbook is looked up for conversation_id "C3"
    Then the resume lookup returns no open playbook

  Scenario: two open playbooks for one conversation resume the most-recently-begun
    Given a resume adapter with track "20260601T0003_early" kind "track" state "spec" conversation_id "C4" begun at "2026-06-01T10:00:00Z"
    And a resume adapter with track "20260601T0004_late" kind "track" state "plan" conversation_id "C4" begun at "2026-06-01T12:00:00Z"
    When the open playbook is looked up for conversation_id "C4"
    Then the resume lookup returns artifact "20260601T0004_late" kind "track" state "plan"

  Scenario: the most-recently-begun selection ignores insertion order
    Given a resume adapter with track "20260601T0005_late" kind "track" state "plan" conversation_id "C5" begun at "2026-06-01T15:00:00Z"
    And a resume adapter with track "20260601T0006_early" kind "track" state "spec" conversation_id "C5" begun at "2026-06-01T09:00:00Z"
    When the open playbook is looked up for conversation_id "C5"
    Then the resume lookup returns artifact "20260601T0005_late" kind "track" state "plan"
