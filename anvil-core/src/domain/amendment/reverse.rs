//! Reversal by identity (AC-5).
//!
//! `reverse(log, op_id) -> OpLog` returns a NEW log with the entry identified by
//! `op_id` removed; re-folding the result returns the projection to its pre-op
//! state. Pure function over the log; reversal keys on `op_id` (KD-4). No prose
//! reversal authored.
//!
//! Surviving entries keep their original `op_id`, `accepted_at`, and `seq`, so
//! `ordered()` over the reversed log is the original order minus the removed
//! entry — the fold is identical to "as if that op had never been accepted."

use crate::domain::amendment::op::OpLog;

/// Return a new log with the entry identified by `op_id` removed.
pub fn reverse(log: &OpLog, op_id: &str) -> OpLog {
    let mut reversed = OpLog::new();
    for entry in log.entries() {
        if entry.op_id != op_id {
            // Preserve the original identity + ordering keys (not push's seq).
            reversed.push_entry(entry.clone());
        }
    }
    reversed
}
