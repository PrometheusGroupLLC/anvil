Feature: Resume-aware routing resolves a conversation's open playbook
  resume_aware_routing Phase 2 — given a conversation_id, the engine helper
  scans the active artifacts' begin markers and returns the open (begun, not
  terminal) playbook for that conversation, picking the most-recently-begun one
  when several are open. Completed (terminal) playbooks are never returned, and a
  different conversation's open playbook is never returned. The fixture machines
  declare a non-terminal "active" state and a terminal "completed" state.

  Background:
    Given a routable playbooks fixture registry with authored route triggers

  Scenario: a single open playbook for the conversation resolves
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    When the open-playbook lookup runs for conversation "C1"
    Then the open-playbook lookup resolves artifact "20260623T0001_alpha" kind "track" state "active"

  Scenario: two open playbooks for one conversation resolve the most-recently-begun
    Given an open playbook "20260623T0001_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    And an open playbook "20260623T0002_beta" of kind "daily_recap" in state "active" begun for conversation "C1" at "2026-06-23T06:00:00Z"
    When the open-playbook lookup runs for conversation "C1"
    Then the open-playbook lookup resolves artifact "20260623T0002_beta" kind "daily_recap" state "active"

  Scenario: a completed playbook is not treated as open
    Given an open playbook "20260623T0003_done" of kind "track" in state "completed" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    When the open-playbook lookup runs for conversation "C1"
    Then the open-playbook lookup resolves nothing

  Scenario: another conversation's open playbook is not returned
    Given an open playbook "20260623T0004_other" of kind "track" in state "active" begun for conversation "C2" at "2026-06-23T05:00:00Z"
    When the open-playbook lookup runs for conversation "C1"
    Then the open-playbook lookup resolves nothing

  Scenario: an empty conversation_id never resolves an open playbook
    Given an open playbook "20260623T0005_alpha" of kind "track" in state "active" begun for conversation "C1" at "2026-06-23T05:00:00Z"
    When the open-playbook lookup runs for conversation ""
    Then the open-playbook lookup resolves nothing
