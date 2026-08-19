Feature: Begin reviewer flow serves the (spec_review, reviewer) hook into review_context_text
  Per AC-1/AC-6 of the hook_content_serving track, `begin(identifier, session_role:"reviewer")`
  on a track in `spec_review` resolves the `track` machine's `spec_review` state `(reviewer)`
  hook via the `PlaybookRegistry`, reads its body via `QueryPort::read_playbook_hook_body`,
  and serves that body verbatim through `review_context_text`. The hook declaration lives on
  the playbook machine (registry), not hardcoded in the handler. `artifact_text` (spec.md)
  is read independently and must remain byte-for-byte unchanged (AC-6).

  Per AC-2, when the `spec_review` state declares no `(reviewer)` hook, `review_context_text`
  is empty and the begin call still succeeds — the absence of a declared hook is not an error.
  This is the symmetric counterpart to the no-doer-hook scenario in `begin_create_serves_hook.feature`.

  Scenario: begin reviewer serves the declared (spec_review, reviewer) hook body verbatim
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec_review" seeded with reviewer hook and spec content "SPEC-BODY-CONTENT"
    And the in-memory query adapter has a hook body for playbook "20260422T0000_track_lifecycle" filename "spec-review.md" with content "DISTINCTIVE-SPEC-REVIEW-HOOK-BODY"
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and a registry declaring spec_review reviewer hook "spec-review.md"
    Then the begin outcome is successful
    And the begin outcome result review_context_text contains "DISTINCTIVE-SPEC-REVIEW-HOOK-BODY"
    And the begin outcome result artifact_text contains "SPEC-BODY-CONTENT"

  Scenario: begin reviewer leaves review_context_text empty when no (spec_review, reviewer) hook is declared
    Given an in-memory query adapter with track "20260414T0405_review_spec_strand" in state "spec_review" seeded with reviewer hook and spec content "SPEC-BODY-CONTENT"
    When begin is called via query adapter with identifier "20260414T0405_review_spec_strand" and a registry declaring no spec_review reviewer hook
    Then the begin outcome is successful
    And the begin outcome result review_context_text is empty
    And the begin outcome result artifact_text contains "SPEC-BODY-CONTENT"
