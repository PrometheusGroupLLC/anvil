Feature: Snapshot filesystem proposal transition preserves intent.md structure
  intent.md uses H2 groups (`## Proposals`, `## Milestones`) with H3
  state sections (`### Vision (N)`, `### Draft (N)`, `### Active (N)`)
  and bullet-list rows (`- **Name** — summary.`). The FS adapter must
  preserve this convention rather than synthesising tables or leaking
  fine-grained labels through from the domain.

  Scenario: Proposal vision_review → draft updates intent.md with consolidated labels
    Given a snapshot fs hearth with:
      | path                                          | content                                                                                                                                                                                                                                                                                                                                                                                                                                 |
      | proposals/20260417T0800_sample_prop/status.yaml | version: 1\nkind: proposal\nstate: vision_review\nactors: {}\ntransitions: []\n                                                                                                                                                                                                                                                                                                                                                           |
      | proposals/20260417T0800_sample_prop/vision.md | # Sample Proposal\n\nVision body.                                                                                                                                                                                                                                                                                                                                                                                                         |
      | proposals.md                                  | # Proposals\n\n## vision\n\n- [Sample Proposal](proposals/20260417T0800_sample_prop/) — sample proposal\n                                                                                                                                                                                                                                                                                                                                  |
      | projections/intent.md                         | ---\nincremental_count: 0\nbase_snapshot: 2026-04-16T00:00:00Z\nlast_updated: 2026-04-16T00:00:00Z\nafter_event: ""\n---\n\n# Anvil — State of Intent\n\n## Proposals\n\n### Vision (1)\n\n- **Sample Proposal** — sample proposal summary.\n\n### Draft (0)\n\n### Active (0)\n                                                                                                                                                          |
    When snapshot fs is executed with:
      | artifact_path  | proposals/20260417T0800_sample_prop |
      | to_state       | proposal                            |
      | actor_name     | Reviewer-555555                     |
      | actor_role     | propose                             |
      | actor_type     | agent                               |
      | actor_model    | claude-opus-4-7                     |
      | actor_provider | anthropic                           |
      | at             | 2026-04-17T08:00:00Z                |
    Then the snapshot result is successful
    And the snapshot result projections_updated contains "intent.md"
    And the file "projections/intent.md" contains "## Proposals"
    And the file "projections/intent.md" contains "### Vision (0)"
    And the file "projections/intent.md" contains "### Draft (1)"
    And the file "projections/intent.md" contains "- **Sample Proposal**"
    And the file "projections/intent.md" does not contain "| Sample Proposal"
    And the file "projections/intent.md" does not contain "### Proposal ("
