# Retire Mode — Mark an Initiative as No Longer Enforced

Retire an initiative. The initiative is kept for history but is no longer enforced. Requires human approval.

The `snapshot` MCP tool transition to `retired` was already fired by the protocol.

## Pre-condition

The initiative must be in `active` or `promoted` state.

**If the initiative is not in `active` or `promoted` state:** Report the actual state to the human and ask how to proceed. Do not attempt the transition — this is a hard gate, not a warning.

## Steps

1. If the initiative is `promoted`, remove the corresponding CLAUDE.md rule first. Present the change before applying — same interaction as demote mode's CLAUDE.md step.
2. **Update `definition.md`:** change Status to `retired`
3. **Append evidence event** directly to `evidence.md`:
   ```markdown
   #### {timestamp from `date -u +%Y-%m-%dT%H:%M:%SZ`} — retired
   Initiative retired. {Reason — e.g., fully converged, no longer relevant, superseded by X}.
   ```
4. **Commit:** `initiative(forge): retire {name}`
