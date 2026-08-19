//! Data-dir-keyed singleton lock — prevents a SECOND engine from binding on the
//! same data directory.
//!
//! The lock is keyed on the **data directory** (`<data_dir>/engine.lock`), NOT
//! on a port. This is deliberate:
//!
//! - Foundry assigns engines a **dynamic** port, so a port-keyed lock would
//!   fight that allocation.
//! - Brine (and any test harness) runs isolated engines on **distinct temp
//!   data-dirs**; a port- or host-global lock would make those block each
//!   other. Keying on the data-dir means two engines on different data-dirs
//!   never contend, while two engines pointed at the SAME data-dir (the real
//!   orphan-creating bug) cannot both run.
//!
//! Acquisition is a non-blocking `flock(LOCK_EX | LOCK_NB)` on a lock file. The
//! holder writes its pid into the file for diagnostics; a second acquirer reads
//! that pid back into [`SingletonError::AlreadyRunning`]. The returned
//! [`SingletonGuard`] holds the open file descriptor and releases the lock on
//! `Drop` (closing the fd drops the flock).
//!
//! On non-Unix targets (we ship macOS + the CI Linux targets, all Unix) the
//! acquire is a stub that always succeeds — there is nothing to gate.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// Filename, under the data dir, holding the singleton lock.
pub const SINGLETON_LOCK_FILENAME: &str = "engine.lock";

/// Error returned by [`acquire_singleton`].
#[derive(Debug)]
pub enum SingletonError {
    /// Another live engine already holds the lock for this data-dir. The holder
    /// pid is read from the lock file (or a sibling `engine.json`) when
    /// available — `None` means the lock is held but we could not read a pid.
    AlreadyRunning { holder_pid: Option<u32> },
    /// The lock file could not be opened/created (permissions, missing parent,
    /// etc.). Carries the underlying IO error.
    Io(io::Error),
}

impl std::fmt::Display for SingletonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SingletonError::AlreadyRunning { holder_pid } => match holder_pid {
                Some(pid) => write!(
                    f,
                    "another engine is already running for this data dir (holder pid {pid})"
                ),
                None => write!(f, "another engine is already running for this data dir"),
            },
            SingletonError::Io(e) => write!(f, "singleton lock io error: {e}"),
        }
    }
}

impl std::error::Error for SingletonError {}

/// Holds the singleton lock for the lifetime of the engine. Dropping the guard
/// closes the file descriptor, which releases the `flock` so a later engine on
/// the same data-dir can acquire it.
#[derive(Debug)]
pub struct SingletonGuard {
    /// Held open for the lock's lifetime. `None` only on non-Unix stubs.
    _file: Option<File>,
    /// The lock path, kept for diagnostics/logging.
    path: PathBuf,
}

impl SingletonGuard {
    /// The `<data_dir>/engine.lock` path this guard holds.
    pub fn lock_path(&self) -> &Path {
        &self.path
    }
}

/// Acquire the data-dir singleton lock.
///
/// Creates `<data_dir>` if needed, opens `<data_dir>/engine.lock`, and takes a
/// non-blocking exclusive `flock`. On success the holder pid is written into the
/// lock file and a [`SingletonGuard`] is returned (hold it for the engine's
/// lifetime). If the lock is already held by a live engine, returns
/// [`SingletonError::AlreadyRunning`] with the holder pid when readable.
///
/// Two acquisitions on DIFFERENT data-dirs never contend.
#[cfg(unix)]
pub fn acquire_singleton(data_dir: &Path) -> Result<SingletonGuard, SingletonError> {
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::io::AsRawFd;

    std::fs::create_dir_all(data_dir).map_err(SingletonError::Io)?;
    let path = data_dir.join(SINGLETON_LOCK_FILENAME);

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(SingletonError::Io)?;

    // Non-blocking exclusive lock. EWOULDBLOCK => already held.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = io::Error::last_os_error();
        // `flock` reports a held lock with EWOULDBLOCK. On some targets
        // EWOULDBLOCK == EAGAIN (same value); accept either without listing both
        // literally (that would be an unreachable-pattern lint when equal).
        let code = err.raw_os_error();
        if code == Some(libc::EWOULDBLOCK) || code == Some(libc::EAGAIN) {
            // Already locked — read the holder pid for diagnostics: first from
            // the lock file's contents, falling back to a sibling engine.json.
            let holder_pid =
                read_pid_from_lock(&mut file).or_else(|| read_pid_from_engine_json(data_dir));
            return Err(SingletonError::AlreadyRunning { holder_pid });
        }
        return Err(SingletonError::Io(err));
    }

    // We hold the lock. Stamp our pid in for the next acquirer's diagnostics.
    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.set_len(0);
    let _ = write!(file, "{}", std::process::id());
    let _ = file.flush();

    Ok(SingletonGuard {
        _file: Some(file),
        path,
    })
}

/// Read a pid the prior holder stamped into the lock file. Best-effort; any
/// parse/io failure yields `None`.
#[cfg(unix)]
fn read_pid_from_lock(file: &mut File) -> Option<u32> {
    use std::io::{Read, Seek, SeekFrom};
    let _ = file.seek(SeekFrom::Start(0));
    let mut s = String::new();
    file.read_to_string(&mut s).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok()
}

/// Fall back to the pid recorded in a sibling `engine.json` (the rendezvous
/// record) when the lock file carried no readable pid.
#[cfg(unix)]
fn read_pid_from_engine_json(data_dir: &Path) -> Option<u32> {
    let bytes = std::fs::read(data_dir.join(crate::writer::RENDEZVOUS_FILENAME)).ok()?;
    let record: crate::record::RendezvousRecord = serde_json::from_slice(&bytes).ok()?;
    Some(record.pid)
}

/// Non-Unix stub: there is no flock to take, so acquisition always succeeds.
/// Our shipping + CI targets are all Unix; this exists only so the crate builds
/// everywhere.
#[cfg(not(unix))]
pub fn acquire_singleton(data_dir: &Path) -> Result<SingletonGuard, SingletonError> {
    std::fs::create_dir_all(data_dir).map_err(SingletonError::Io)?;
    Ok(SingletonGuard {
        _file: None,
        path: data_dir.join(SINGLETON_LOCK_FILENAME),
    })
}
