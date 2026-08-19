# Promote / Demote Mode — CLAUDE.md Rule Management

These are structural mirrors: promote adds a CLAUDE.md rule, demote removes one. Both require human approval.

The `snapshot` MCP tool transition was already fired by the protocol.

## Pre-condition

| Mode | Required state | Target state |
|------|---------------|--------------|
| promote | `active` | `promoted` |
| demote | `promoted` | `active` |

**If the initiative is not in the required state:** Report the actual state to the human and ask how to proceed. Do not attempt the transition — this is a hard gate, not a warning.

## Shared steps

1. **Read** the initiative's `evidence.md` and `definition.md`
   - For promote: confirm convergence evidence supports promotion
   - For demote: understand why demotion is warranted (too many exceptions, fighting the grain)

2. **Update `definition.md`:**
   - Change the Status field to the target state
   - For promote: add "Related CLAUDE.md rule" reference
   - For demote: remove or note "Related CLAUDE.md rule" as demoted

3. **Modify CLAUDE.md** Rules section:
   - For promote: **add** the rule. The rule text should be concise and consistent with existing rules. Present the proposed rule text to the human before adding.
   - For demote: **remove** the rule. Present the change before applying.

4. **Append evidence event** directly to `evidence.md`:
   ```markdown
   #### {timestamp from `date -u +%Y-%m-%dT%H:%M:%SZ`} — promoted
   Initiative promoted to CLAUDE.md rule. Evidence supported consistent convergence across N tracks.
   ```
   or:
   ```markdown
   #### {timestamp from `date -u +%Y-%m-%dT%H:%M:%SZ`} — demoted
   Initiative demoted from CLAUDE.md rule. {Reason — e.g., persistent exceptions, pattern needs refinement}.
   ```

5. **Commit:**
   - Promote: `initiative(forge): promote {name} — add CLAUDE.md rule`
   - Demote: `initiative(forge): demote {name} — remove CLAUDE.md rule`
