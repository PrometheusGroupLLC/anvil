Feature: Agent hook paths resolve

  Agent-facing instructions — the shipped skills and the playbook hook bodies —
  name hook files by path. Those paths are resolved against STAGED KIT CONTENT,
  not against the source tree: a `playbooks/<id>/` directory without a
  `machine.yaml` is never packaged, installed, or registered, so a hook path
  inside one is dead on a user's machine even though the file exists in this
  repo. Staging goes through `scripts/stage-kit-content.sh`, the same script
  `scripts/build-kit.sh` uses to assemble the published kit.

  Scenario: Agent instructions point to live playbook hooks
    Given the shipped skills and source playbooks
    When every documented hook path is resolved
    Then each current path exists below the playbooks directory

  Scenario: Staged kit content is not the source tree
    Given the shipped skills and source playbooks
    Then the staged playbooks are exactly the source playbooks that carry a machine

  # The Claude Code slash-command prompts are NOT kit content — they live in the
  # repository and brine-private/.claude/commands/forge symlinks this same tree,
  # so both projects serve them to every agent. They carry the same hook-path
  # instructions the shipped skills do, and before this track they pointed at a
  # `workflows/` directory that does not exist. They resolve against the SOURCE
  # playbooks tree, which is what an agent working in this repo actually reads.
  Scenario: Slash-command prompts point to live playbook hooks
    Given the repository's Claude Code slash-command prompts
    Then each documented slash-command hook path exists below the source playbooks directory
