//! `anvil-change-record-import` — establish a hearth's change-record baseline.
//!
//! Stage A of the change record, and the only act in the whole mechanism that
//! is allowed to run `git init`. It is a one-shot, re-runnable act: a second
//! run on an already-imported hearth establishes no second baseline and creates
//! no duplicate commit, because the ref update is a create-only
//! compare-and-swap and git refuses it rather than this binary pre-checking.
//!
//! Deliberately a SEPARATE binary from `anvil-change-record-report`, which is
//! required to write nothing. Import must write a commit, so one binary cannot
//! honour both sentences.
//!
//! It never moves `HEAD`, never stages into the repository's own index, never
//! modifies a file it did not write, and never touches a network remote.
//!
//! Usage:
//!
//! ```text
//! anvil-change-record-import --hearth <path>
//! ```
//!
//! Exit codes: `0` on an established or already-established baseline, `2` on a
//! usage error, `3` on a refusal — which names what was found rather than
//! importing into the wrong repository.

use anvil_core_hearth::change_record_baseline::import_baseline;
use std::path::PathBuf;

fn main() {
    let mut hearth: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--hearth" => hearth = args.next().map(PathBuf::from),
            other => {
                eprintln!("anvil-change-record-import: unknown argument {other:?}");
                eprintln!("usage: anvil-change-record-import --hearth <path>");
                std::process::exit(2);
            }
        }
    }
    let Some(hearth) = hearth else {
        eprintln!("anvil-change-record-import: --hearth <path> is required");
        std::process::exit(2);
    };

    match import_baseline(&hearth) {
        Ok(import) if import.already_imported => {
            println!(
                "{} already has a baseline at {} ({} recorded path(s) on disk). Nothing written.",
                hearth.display(),
                import.baseline,
                import.paths_recorded
            );
        }
        Ok(import) => {
            println!(
                "{} imported: baseline {} over {} recorded path(s), on refs/anvil/baseline and \
                 refs/anvil/change-record.",
                hearth.display(),
                import.baseline,
                import.paths_recorded
            );
        }
        Err(refusal) => {
            eprintln!("anvil-change-record-import: {}", refusal);
            std::process::exit(3);
        }
    }
}
