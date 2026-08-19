Feature: SessionMode decides the operating mode from the helper's tri-state
  The single seam (spec Req 2) that translates the helper's tri-state outcome
  into a `SessionMode` value. No JWT logic lives here — the mapping is a
  pure decision over the outcome enum and whether a token was present.

  Scenario: Absent token always yields Standalone regardless of outcome
    Given the verify outcome is Ok(None)
    And the raw token is absent
    When session mode is decided
    Then the session mode is Standalone

  Scenario: Whitespace-only token is treated as absent — yields Standalone
    Given the verify outcome is Ok(None)
    And the raw token is whitespace-only
    When session mode is decided
    Then the session mode is Standalone

  Scenario: Ok(None) with a non-empty token yields Standalone
    Given the verify outcome is Ok(None)
    And the raw token is present
    When session mode is decided
    Then the session mode is Standalone

  Scenario: Ok(Some(session)) yields Foundry carrying the session
    Given the verify outcome is Ok(Some(session)) with sub "user-42" and sid "sess-1"
    And the raw token is present
    When session mode is decided
    Then the session mode is Foundry with sub "user-42"

  Scenario: Err(_) with present token yields Refuse
    Given the verify outcome is Err(BadSignature)
    And the raw token is present
    When session mode is decided
    Then the session mode is Refuse

  Scenario: Err(_) with whitespace token yields Standalone not Refuse
    Given the verify outcome is Err(BadSignature)
    And the raw token is whitespace-only
    When session mode is decided
    Then the session mode is Standalone
