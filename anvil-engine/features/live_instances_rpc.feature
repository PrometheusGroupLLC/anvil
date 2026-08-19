Feature: LiveInstances RPC surfaces open instances with their RAW live actor
  The read-only LiveInstances query backs the in-app Atlas "live actors" overlay.
  It folds the OPEN (non-terminal) artifact instances in the hearth into one row
  each — the instance id, its specific kind, its FOLDED current state, and the RAW
  actor name + timestamp taken from that instance's `activity:` begin-markers (the
  SAME LOCAL-ONLY signal ActorActivity reads). It NEVER reads the salted
  `actor_hash` in the redacted activity-log — surfacing a salted hash as a "live
  actor" is exactly the dishonesty the Atlas exists to expose. A begun-not-completed
  instance appears with its raw actor; an instance whose current state is terminal
  is dropped. The gRPC RPC and the /ws `live_instances` method fold the SAME core
  path, so the two surfaces can never diverge.

  Scenario: the gRPC LiveInstances RPC returns an open instance with its folded state + RAW actor
    Given a live instances engine hearth
    And the engine is started with that hearth
    When the LiveInstances RPC is called
    Then live instances has an instance "20260707T0000_open_track"
    And the live instance "20260707T0000_open_track" has kind "track"
    And the live instance "20260707T0000_open_track" has state "spec"
    And the live instance "20260707T0000_open_track" has actor "Atlas-Doer-4211"
    And the live instance "20260707T0000_open_track" actor is a raw name not a hash

  Scenario: a terminal instance is dropped from the live view
    Given a live instances engine hearth
    And the engine is started with that hearth
    When the LiveInstances RPC is called
    Then live instances has no instance "20260707T0100_done_track"

  Scenario: the /ws live_instances method folds the identical open set
    Given a live instances engine hearth
    And the engine is started with that hearth
    When a live_instances JSON-RPC request is sent over /ws with hearth_path ""
    Then the /ws live_instances result has an instance "20260707T0000_open_track"
    And the /ws live_instances instance "20260707T0000_open_track" has actor "Atlas-Doer-4211"
    And the /ws live_instances result has no instance "20260707T0100_done_track"

  Scenario: an open-but-dormant instance (no actor, no §0 activity) is excluded and counted honestly, not rendered as a fake live agent
    Given a live instances engine hearth
    And the engine is started with that hearth
    And a dormant open track "20260707T0300_dormant_track" exists in that hearth
    When the LiveInstances RPC is called
    Then live instances has no instance "20260707T0300_dormant_track"
    And live instances has idle_count 1
    # The genuinely-live instance from the base fixture is unaffected.
    And live instances has an instance "20260707T0000_open_track"

  Scenario: a live instance carries its real per-instance detail — current_step, action_count, artifact_dir
    Given a live instances engine hearth
    And the engine is started with that hearth
    And the temper step0 stream for kind "track" has 3 events for instance "20260707T0000_open_track"
    When the LiveInstances RPC is called
    Then the live instance "20260707T0000_open_track" has current_step "spec"
    And the live instance "20260707T0000_open_track" has action_count 3
    And the live instance "20260707T0000_open_track" has artifact_dir ending with "tracks/20260707T0000_open_track"
