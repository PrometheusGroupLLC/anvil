Feature: Evidence step measurements are written fail-open
  Evidence assessment is durable telemetry, not a lifecycle gate. A broken or
  slow step-measurement writer must never prevent the selected transition from
  returning or becoming visible, while rows accepted by the writer retain
  request order and are delivered at most once.

  Scenario Outline: a blocked evidence sink does not fail a lifecycle transition
    Given a P2b obligated playbook hearth prepared for "<lifecycle>"
    And the evidence step-measurement sink is blocked by a directory
    And the engine is started with that hearth
    When the P2b "<lifecycle>" lifecycle RPC is called
    Then the P2b lifecycle RPC succeeds in state "<state>"
    And state "<state>" is visible in the transitioned artifact

    # AC10 (T-ACT-2 P4-S7 nit): "complete_cli" routes through the real
    # `anvil-hooks complete --claimed-evidence ...` CLI affordance (not an
    # in-process RPC construction), re-asserting fail-open now that this
    # track is the first to route real traffic through the affordance.
    Examples:
      | lifecycle    | state       |
      | begin        | spec        |
      | snapshot     | spec_review |
      | complete     | spec_review |
      | complete_cli | spec_review |

  Scenario Outline: paused durable evidence delivery does not delay lifecycle progress
    Given an obligated playbook is ready for a "<lifecycle>" transition with durable evidence delivery paused
    When the "<lifecycle>" transition is requested before durable evidence delivery resumes
    Then the transition returns in state "<state>" while durable evidence delivery remains paused
    And state "<state>" is durably visible before evidence delivery resumes
    When durable evidence delivery resumes
    Then exactly one delivered evidence row records "<from>" to "<state>"

    Examples:
      | lifecycle | from | state       |
      | begin     |      | spec        |
      | snapshot  | spec | spec_review |
      | complete  | spec | spec_review |

  Scenario: a parked evidence writer leaves lifecycle progress responsive and drains FIFO exactly once
    Given a P2b obligated playbook hearth prepared for "begin"
    And the engine is started with its evidence step-measurement writer parked
    When the parking probe begin RPC is called before writer release
    Then the begin RPC returns with state "spec" visible while the evidence writer is parked
    When the parking probe complete RPC is called before writer release
    Then the complete RPC returns with state "spec_review" visible while the evidence writer is parked
    When the parked evidence writer is released
    Then its delivered evidence rows drain in lifecycle order exactly once

  Scenario: a parked evidence writer leaves snapshot progress responsive and drains exactly once
    Given a P2b obligated playbook hearth prepared for "snapshot"
    And the engine is started with its evidence step-measurement writer parked
    When the parking probe snapshot RPC is called before writer release
    Then the snapshot RPC returns with state "spec_review" visible while the evidence writer is parked
    When the parked evidence writer is released
    Then its delivered snapshot evidence row drains exactly once
