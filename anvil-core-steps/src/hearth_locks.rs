//! Step definitions for the `HearthLocks` per-hearth write-lock primitive
//! (`anvil_core_hearth::hearth_locks::HearthLocks`), exercised at the
//! anvil-core library seam (Slice A — no engine subprocess).
//!
//! The serialization guarantee is proven deterministically and in-process: two
//! writers perform a read-modify-write of a shared counter, where writer A
//! acquires its hearth guard, reads, and PARKS (after read / before write) on
//! an injected barrier. Writer B then runs its full read-modify-write against
//! the same hearth. The test resumes A only once B has entered its `lock_for`
//! acquisition.
//!
//! - With a working per-hearth lock: B blocks in `lock_for` until A commits and
//!   releases, so B observes A's committed value → final counter == 2 (no lost
//!   update). A is resumed before it would deadlock (the test never waits for B
//!   to finish before resuming A).
//! - Without the lock (RED): B reads the stale value while A is parked → both
//!   write the same+1 → final counter == 1 (lost update).
//!
//! A companion scenario drives two writers on DIFFERENT hearths and asserts B
//! does not block on A (it completes while A is still parked).
//!
//! The "no meta-mutex inversion" scenario (A3) parks A holding hearth X's guard
//! and asserts a fresh `lock_for(Y)` still completes promptly — proving the
//! meta-mutex guarding the map is released before the per-hearth lock is held.

use anvil_core_hearth::hearth_locks::HearthLocks;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{async_step_def, check_def, StepDef};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// Outcome of a same-hearth serialization run: the final committed counter
/// value after both writers complete.
#[derive(Clone)]
struct SerializationOutcome {
    final_value: i64,
}

/// Outcome of a different-hearth (or no-inversion) run: whether the second
/// operation completed while the first writer was still parked.
#[derive(Clone)]
struct NonBlockingOutcome {
    second_completed_while_first_parked: bool,
}

async fn yield_until(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        async_step_def(
            "a HearthLocks primitive",
            &[],
            &[("hearth_locks", "PathBuf")],
            |_ctx, _params| async move {
                // We cannot stash the live HearthLocks in the brine Context
                // (it is not Clone), so each run step constructs its own. We
                // record a marker so the feature reads naturally.
                let mut out = Context::new();
                out.set("hearth_locks", PathBuf::from("hearth_locks"));
                Ok(out)
            },
        ),
        // ===== A2: same-hearth serialization (no lost update) =====
        async_step_def(
            "two writers race a read-modify-write on the same hearth through HearthLocks",
            &[("hearth_locks", "PathBuf")],
            &[("serialization_outcome", "SerializationOutcome")],
            |mut ctx, _params| async move {
                let _ = ctx.take::<PathBuf>("hearth_locks");
                let locks = Arc::new(HearthLocks::new());
                let counter = Arc::new(tokio::sync::Mutex::new(0i64));
                let hearth = PathBuf::from("/tmp/hearth-X");

                let a_parked = Arc::new(Notify::new());
                let a_resume = Arc::new(Notify::new());
                let b_attempting = Arc::new(AtomicBool::new(false));

                // Writer A: acquire, read, park (after read / before write).
                let task_a = {
                    let locks = locks.clone();
                    let counter = counter.clone();
                    let hearth = hearth.clone();
                    let a_parked = a_parked.clone();
                    let a_resume = a_resume.clone();
                    tokio::spawn(async move {
                        let _guard = locks.lock_for(&hearth).await;
                        let read = *counter.lock().await;
                        a_parked.notify_one();
                        a_resume.notified().await;
                        *counter.lock().await = read + 1;
                        // guard dropped here
                    })
                };

                // Wait until A has read and parked.
                a_parked.notified().await;

                // Writer B: full read-modify-write on the same hearth.
                let task_b = {
                    let locks = locks.clone();
                    let counter = counter.clone();
                    let hearth = hearth.clone();
                    let b_attempting = b_attempting.clone();
                    tokio::spawn(async move {
                        b_attempting.store(true, Ordering::SeqCst);
                        let _guard = locks.lock_for(&hearth).await;
                        let read = *counter.lock().await;
                        *counter.lock().await = read + 1;
                    })
                };

                // Resume A only once B has entered its lock_for acquisition.
                yield_until(&b_attempting).await;
                // Give B a chance to reach the (blocking) lock acquisition.
                for _ in 0..16 {
                    tokio::task::yield_now().await;
                }
                a_resume.notify_one();

                task_a.await.map_err(|e| format!("task A join: {}", e))?;
                task_b.await.map_err(|e| format!("task B join: {}", e))?;

                let final_value = *counter.lock().await;
                let mut out = Context::new();
                out.set(
                    "serialization_outcome",
                    SerializationOutcome { final_value },
                );
                Ok(out)
            },
        ),
        check_def(
            "the final committed value is {int}",
            &[("serialization_outcome", "SerializationOutcome")],
            |ctx, params| {
                let expected = params.get_int(0).ok_or("Expected value")?;
                let outcome = ctx
                    .get::<SerializationOutcome>("serialization_outcome")
                    .ok_or("No serialization_outcome")?;
                if outcome.final_value == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected final committed value {} (no lost update), got {}",
                        expected, outcome.final_value
                    ))
                }
            },
        ),
        // ===== A2: different-hearth non-blocking =====
        async_step_def(
            "a writer parks holding one hearth's lock and a second writer locks a different hearth",
            &[("hearth_locks", "PathBuf")],
            &[("nonblocking_outcome", "NonBlockingOutcome")],
            |mut ctx, _params| async move {
                let _ = ctx.take::<PathBuf>("hearth_locks");
                run_two_hearth_probe(PathBuf::from("/tmp/hearth-X"), PathBuf::from("/tmp/hearth-Y"))
                    .await
            },
        ),
        // ===== A3: no meta-mutex inversion =====
        // Identical mechanism to the different-hearth probe — A parks holding
        // X's guard, and a fresh lock_for(Y) must still complete promptly. If
        // lock_for held the meta-mutex across the per-hearth acquisition, the
        // Y acquisition would block behind A.
        async_step_def(
            "a writer parks holding hearth X's lock and a fresh lock_for on hearth Y is requested",
            &[("hearth_locks", "PathBuf")],
            &[("nonblocking_outcome", "NonBlockingOutcome")],
            |mut ctx, _params| async move {
                let _ = ctx.take::<PathBuf>("hearth_locks");
                run_no_inversion_probe().await
            },
        ),
        check_def(
            "the second writer completes while the first is still parked",
            &[("nonblocking_outcome", "NonBlockingOutcome")],
            |ctx, _params| {
                let outcome = ctx
                    .get::<NonBlockingOutcome>("nonblocking_outcome")
                    .ok_or("No nonblocking_outcome")?;
                if outcome.second_completed_while_first_parked {
                    Ok(())
                } else {
                    Err("Expected the second writer to complete while the first \
                         was still parked (different hearth must not block), but it \
                         did not"
                        .to_string())
                }
            },
        ),
        // ===== A4: begin-style acquire-once-hold-across-writes =====
        // A single guard acquired before the read is held across N writes; an
        // interleaving same-hearth writer cannot slip between the read and the
        // writes. Reuses the serialization machinery but A performs TWO writes
        // while holding one guard.
        async_step_def(
            "one writer holds a single hearth guard across a read and multiple writes while another same-hearth writer races",
            &[("hearth_locks", "PathBuf")],
            &[("serialization_outcome", "SerializationOutcome")],
            |mut ctx, _params| async move {
                let _ = ctx.take::<PathBuf>("hearth_locks");
                let locks = Arc::new(HearthLocks::new());
                let counter = Arc::new(tokio::sync::Mutex::new(0i64));
                let hearth = PathBuf::from("/tmp/hearth-X");

                let a_parked = Arc::new(Notify::new());
                let a_resume = Arc::new(Notify::new());
                let b_attempting = Arc::new(AtomicBool::new(false));

                // Writer A: acquire ONCE, read, park, then perform two writes
                // while still holding the same guard (a begin-style multi-write
                // transaction).
                let task_a = {
                    let locks = locks.clone();
                    let counter = counter.clone();
                    let hearth = hearth.clone();
                    let a_parked = a_parked.clone();
                    let a_resume = a_resume.clone();
                    tokio::spawn(async move {
                        let _guard = locks.lock_for(&hearth).await;
                        let read = *counter.lock().await;
                        a_parked.notify_one();
                        a_resume.notified().await;
                        // Two writes under the SAME guard.
                        *counter.lock().await = read + 1;
                        let read2 = *counter.lock().await;
                        *counter.lock().await = read2 + 1;
                    })
                };

                a_parked.notified().await;

                let task_b = {
                    let locks = locks.clone();
                    let counter = counter.clone();
                    let hearth = hearth.clone();
                    let b_attempting = b_attempting.clone();
                    tokio::spawn(async move {
                        b_attempting.store(true, Ordering::SeqCst);
                        let _guard = locks.lock_for(&hearth).await;
                        let read = *counter.lock().await;
                        *counter.lock().await = read + 1;
                    })
                };

                yield_until(&b_attempting).await;
                for _ in 0..16 {
                    tokio::task::yield_now().await;
                }
                a_resume.notify_one();

                task_a.await.map_err(|e| format!("task A join: {}", e))?;
                task_b.await.map_err(|e| format!("task B join: {}", e))?;

                let final_value = *counter.lock().await;
                let mut out = Context::new();
                out.set(
                    "serialization_outcome",
                    SerializationOutcome { final_value },
                );
                Ok(out)
            },
        ),
    ]
}

/// No-meta-inversion probe (A3). Writer A holds hearth X's guard and parks. A
/// SECOND contender on hearth X then calls `lock_for(X)` and blocks inside the
/// acquisition (X is held). If `lock_for` held the meta-mutex across the
/// per-hearth `.await`, this blocked X-contender would still be holding the
/// meta-mutex — starving any other `lock_for`. We then issue `lock_for(Y)` and
/// assert it completes while A (and the blocked X-contender) are still parked.
/// Under the correct implementation the meta-guard was dropped before the
/// X-contender's await, so Y proceeds.
async fn run_no_inversion_probe() -> Result<Context, String> {
    let locks = Arc::new(HearthLocks::new());
    let hearth_x = PathBuf::from("/tmp/hearth-X");
    let hearth_y = PathBuf::from("/tmp/hearth-Y");

    let a_parked = Arc::new(Notify::new());
    let a_release = Arc::new(Notify::new());
    let x_contender_started = Arc::new(AtomicBool::new(false));
    let y_done = Arc::new(AtomicBool::new(false));

    // A: acquire X, park.
    let task_a = {
        let locks = locks.clone();
        let hearth_x = hearth_x.clone();
        let a_parked = a_parked.clone();
        let a_release = a_release.clone();
        tokio::spawn(async move {
            let _guard = locks.lock_for(&hearth_x).await;
            a_parked.notify_one();
            a_release.notified().await;
        })
    };
    a_parked.notified().await;

    // X-contender: attempt to lock X (held by A) — blocks inside lock_for.
    let task_x_contender = {
        let locks = locks.clone();
        let hearth_x = hearth_x.clone();
        let started = x_contender_started.clone();
        tokio::spawn(async move {
            started.store(true, Ordering::SeqCst);
            let _guard = locks.lock_for(&hearth_x).await;
        })
    };
    // Let the X-contender reach its (blocking) acquisition.
    yield_until(&x_contender_started).await;
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }

    // Y-locker: a fresh, uncontended hearth. Must complete despite the blocked
    // X-contender.
    let task_y = {
        let locks = locks.clone();
        let y_done = y_done.clone();
        tokio::spawn(async move {
            let _guard = locks.lock_for(&hearth_y).await;
            y_done.store(true, Ordering::SeqCst);
        })
    };

    let mut completed_while_parked = false;
    for _ in 0..1000 {
        if y_done.load(Ordering::SeqCst) {
            completed_while_parked = true;
            break;
        }
        tokio::task::yield_now().await;
    }

    // Release A, then the X-contender can finish; join everything.
    a_release.notify_one();
    task_a.await.map_err(|e| format!("task A join: {}", e))?;
    task_x_contender
        .await
        .map_err(|e| format!("X-contender join: {}", e))?;
    task_y.await.map_err(|e| format!("task Y join: {}", e))?;

    let mut out = Context::new();
    out.set(
        "nonblocking_outcome",
        NonBlockingOutcome {
            second_completed_while_first_parked: completed_while_parked,
        },
    );
    Ok(out)
}

/// Shared machinery for the different-hearth probe: writer A
/// acquires hearth X's guard and parks; a second operation acquires a
/// different hearth's guard and must complete while A is still parked.
async fn run_two_hearth_probe(hearth_x: PathBuf, hearth_y: PathBuf) -> Result<Context, String> {
    let locks = Arc::new(HearthLocks::new());

    let a_parked = Arc::new(Notify::new());
    let a_release = Arc::new(Notify::new());
    let b_done = Arc::new(AtomicBool::new(false));

    let task_a = {
        let locks = locks.clone();
        let a_parked = a_parked.clone();
        let a_release = a_release.clone();
        tokio::spawn(async move {
            let _guard = locks.lock_for(&hearth_x).await;
            a_parked.notify_one();
            a_release.notified().await;
            // guard dropped here
        })
    };

    a_parked.notified().await;

    let task_b = {
        let locks = locks.clone();
        let b_done = b_done.clone();
        tokio::spawn(async move {
            let _guard = locks.lock_for(&hearth_y).await;
            b_done.store(true, Ordering::SeqCst);
        })
    };

    // Give B a bounded window to complete while A is still parked.
    let mut completed_while_parked = false;
    for _ in 0..1000 {
        if b_done.load(Ordering::SeqCst) {
            completed_while_parked = true;
            break;
        }
        tokio::task::yield_now().await;
    }

    // Release A and join.
    a_release.notify_one();
    task_a.await.map_err(|e| format!("task A join: {}", e))?;
    task_b.await.map_err(|e| format!("task B join: {}", e))?;

    let mut out = Context::new();
    out.set(
        "nonblocking_outcome",
        NonBlockingOutcome {
            second_completed_while_first_parked: completed_while_parked,
        },
    );
    Ok(out)
}
