# Log Mode — Append Out-of-Band Evidence

Append an evidence entry for observations outside the normal reflection flow. No state transition — the evidence entry itself is the event.

## Available event types

Only 4 evidence event types are available in log mode:
- `advance` — the initiative's pattern was followed or strengthened
- `regress` — the initiative's pattern was violated or weakened
- `exception-proposed` — an exception to the initiative was proposed
- `exception-approved` — an exception was approved by a human

The other 3 event types (`promoted`, `demoted`, `retired`) are recorded by their respective lifecycle modes, not by log. Do not use log mode to record lifecycle events.

## Steps

1. **Read** the target initiative's `definition.md` to understand what you're logging evidence against
2. **Obtain timestamp** via shell command:
   ```bash
   date -u +%Y-%m-%dT%H:%M:%SZ
   ```
   **Never fabricate a timestamp.** Always use the shell command.
3. **Append entry** directly to the initiative's `evidence.md`:
   ```markdown
   #### {timestamp} — {track-name or context} — {advance|regress|exception-proposed|exception-approved}
   {One-line description of what was observed}
   ```
4. **Commit:** `initiative(forge): log {name} — {event-type}`

No state-transition delegation. The evidence entry is the event — a direct in-directory write to the initiative's own artifact.
