//! Pure folds for the change record. Anything that shells out or touches the
//! filesystem lives in the `anvil-core-hearth` crate instead — no module under `domain/`
//! spawns a subprocess, and this mechanism does not become the first.
//!
//! (Phrased without naming the constructor, so the audit that greps `domain/`
//! for subprocess spawns keeps returning zero files rather than matching this
//! sentence.)

pub mod message;
pub mod paths;
