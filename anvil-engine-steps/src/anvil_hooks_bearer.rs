//! Real-binary steps for `anvil_hooks_foundry_credential.feature`.
//!
//! Drives the REAL `anvil-hooks` binary as a subprocess against a REAL spawned
//! engine, controlling ONLY the CLI's environment. That is the whole seam under
//! test: the engine is unchanged between the success and refusal arms, so a
//! difference in outcome can only come from the credential the CLI presented.
//!
//! Every scenario scrubs `FOUNDRY_SESSION_TOKEN` and `FOUNDRY_BROKER_SOCKET`
//! from the child before setting whatever the scenario is about. Inheriting the
//! developer's own Foundry session would let a "no credential" arm quietly pass
//! WITH one, which is precisely the confusion this feature exists to remove.

use anvil_test_support::engine::EngineProcess;
use brine_runner_rust::context::Context;
use brine_runner_rust::registry::{check_def, step_def, StepDef};
use std::path::PathBuf;
use std::process::Command;

/// What the CLI did: its exit code and its combined output.
#[derive(Clone)]
struct HooksOutcome {
    exit: i32,
    output: String,
    /// The track path the engine reported creating, parsed out of `begin`'s
    /// stdout. Empty when the call was refused — which is itself an assertion
    /// (`anvil-hooks created no track`).
    created_track: String,
}

fn bin() -> PathBuf {
    anvil_test_support::harness::ensure_binary("anvil-hooks");
    anvil_test_support::harness::binary_path("anvil-hooks")
}

/// Build an `anvil-hooks` command with the Foundry credential env SCRUBBED.
///
/// The scrub is not hygiene, it is the control: these scenarios distinguish
/// "presented a credential" from "presented none", and an inherited
/// `FOUNDRY_SESSION_TOKEN` from the developer's shell would collapse the two.
fn hooks_command(hearth: &PathBuf, port: u16, verb: &str) -> Command {
    let mut cmd = Command::new(bin());
    cmd.env_remove("FOUNDRY_SESSION_TOKEN")
        .env_remove("FOUNDRY_BROKER_SOCKET")
        .arg(verb)
        .arg("--hearth")
        .arg(hearth.to_str().unwrap())
        .arg("--port")
        .arg(port.to_string());
    cmd
}

/// Parse the created track path out of `begin`'s success line, which reads
/// ``… → state `spec` at tracks/<ts>_<name>``. The path is timestamped, so it
/// cannot be named in the feature and must be read back from the CLI's own
/// report.
fn parse_created_track(output: &str) -> String {
    output
        .split(" at ")
        .nth(1)
        .map(|rest| rest.lines().next().unwrap_or("").trim().to_string())
        .unwrap_or_default()
}

/// Run `anvil-hooks` and capture the outcome, threading the engine handle back
/// into the context (dropping `EngineProcess` would kill the engine mid-scenario).
fn run_hooks(mut ctx: Context, mut cmd_args: Vec<String>, token: Option<&str>) -> Result<Context, String> {
    let engine = ctx
        .take::<EngineProcess>("engine_process")
        .ok_or("No engine_process")?;
    let hearth = ctx
        .get::<PathBuf>("hearth_path")
        .cloned()
        .ok_or("No hearth_path")?;
    let verb = cmd_args.remove(0);
    let mut cmd = hooks_command(&hearth, engine.port, &verb);
    for a in &cmd_args {
        cmd.arg(a);
    }
    if let Some(t) = token {
        cmd.env("FOUNDRY_SESSION_TOKEN", t);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("run anvil-hooks {verb}: {e}"))?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome = HooksOutcome {
        exit: output.status.code().unwrap_or(-1),
        created_track: parse_created_track(&combined),
        output: combined,
    };

    let mut out = Context::new();
    out.set("engine_process", engine);
    out.set("hearth_path", hearth);
    out.set("hooks_outcome", outcome);
    Ok(out)
}

/// The verb-specific argv, minus the credential and the connection flags. The
/// actor flags are shared, so the two verbs differ only in their leading args
/// and a divergent `--actor-name` the engine is expected to OVERRIDE.
///
/// `begin` supplies `--approver` because the track machine declares it
/// required: without it the call is refused with `missing_required_field`,
/// which would redden the credential assertions for a reason that has nothing
/// to do with the seam under test.
fn hooks_args(verb: &str, target: &str, actor: &str) -> Vec<String> {
    let mut args: Vec<&str> = match verb {
        "begin" => vec![
            "begin",
            "--artifact-type",
            "track",
            "--name",
            "credentialed channel",
            "--parent-id",
            target,
            "--approver",
            "Nick",
        ],
        "complete" => vec![
            "complete",
            "--artifact-path",
            target,
            "--satisfaction",
            "satisfied",
        ],
        other => panic!("unsupported anvil-hooks verb in this feature: {other}"),
    };
    args.extend_from_slice(&[
        "--actor-name",
        actor,
        "--actor-type",
        "agent",
        "--actor-model",
        "test-model",
        "--actor-provider",
        "test-provider",
    ]);
    args.iter().map(|s| s.to_string()).collect()
}

const ENGINE_IN: &[(&str, &str)] = &[("engine_process", "EngineProcess"), ("hearth_path", "PathBuf")];
const ENGINE_OUT: &[(&str, &str)] = &[
    ("engine_process", "EngineProcess"),
    ("hearth_path", "PathBuf"),
    ("hooks_outcome", "HooksOutcome"),
];

fn outcome(ctx: &Context) -> Result<HooksOutcome, String> {
    ctx.get::<HooksOutcome>("hooks_outcome")
        .cloned()
        .ok_or_else(|| "No hooks_outcome".to_string())
}

/// Register one CLI-driving step. `credentialed` selects the arity: the
/// with-token phrasing takes (target, token, actor), the no-credential
/// phrasing takes (target, actor). Keeping both phrasings in the feature is
/// deliberate — "with no Foundry credential" is the control arm and must read
/// as one, not as an empty string argument.
fn cli_step(pattern: &'static str, verb: &'static str, credentialed: bool) -> StepDef {
    step_def(pattern, ENGINE_IN, ENGINE_OUT, move |ctx, params| {
        let target = params.get_string(0).ok_or("Expected target")?.to_string();
        let (token, actor) = if credentialed {
            (
                Some(params.get_string(1).ok_or("Expected token")?.to_string()),
                params.get_string(2).ok_or("Expected actor")?.to_string(),
            )
        } else {
            (None, params.get_string(1).ok_or("Expected actor")?.to_string())
        };
        run_hooks(ctx, hooks_args(verb, &target, &actor), token.as_deref())
    })
}

pub fn steps() -> Vec<StepDef> {
    vec![
        cli_step(
            "anvil-hooks begins a track under parent {string} with session token {string} and actor name {string}",
            "begin",
            true,
        ),
        cli_step(
            "anvil-hooks begins a track under parent {string} with no Foundry credential and actor name {string}",
            "begin",
            false,
        ),
        cli_step(
            "anvil-hooks completes artifact {string} with session token {string} and actor name {string}",
            "complete",
            true,
        ),
        cli_step(
            "anvil-hooks completes artifact {string} with no Foundry credential and actor name {string}",
            "complete",
            false,
        ),
        check_def(
            "the anvil-hooks command succeeds",
            &[("hooks_outcome", "HooksOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.exit == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected anvil-hooks to succeed, got exit {}:\n{}",
                        o.exit, o.output
                    ))
                }
            },
        ),
        check_def(
            "the anvil-hooks command fails",
            &[("hooks_outcome", "HooksOutcome")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if o.exit != 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected anvil-hooks to be REFUSED, but it exited 0:\n{}",
                        o.output
                    ))
                }
            },
        ),
        check_def(
            "the anvil-hooks output contains {string}",
            &[("hooks_outcome", "HooksOutcome")],
            |ctx, params| {
                let needle = params.get_string(0).ok_or("Expected needle")?.to_string();
                let o = outcome(&ctx)?;
                if o.output.contains(&needle) {
                    Ok(())
                } else {
                    Err(format!("Expected output to contain '{needle}':\n{}", o.output))
                }
            },
        ),
        // The created track path is timestamped, so it is read back from the
        // CLI's own success line rather than named in the feature. Reading the
        // PERSISTED transition actor (not the CLI's echo of --actor-name) is
        // what makes the success arm unsatisfiable by an engine that never
        // gated: only the Foundry path rewrites the principal to foundry:<sub>.
        check_def(
            "the anvil-hooks created track transition actor is {string}",
            &[("hooks_outcome", "HooksOutcome"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let expected = params.get_string(0).ok_or("Expected actor")?.to_string();
                let actual = created_track_actor(&ctx)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "Expected persisted transition actor '{expected}', got '{actual}'"
                    ))
                }
            },
        ),
        check_def(
            "the anvil-hooks created track transition actor is not {string}",
            &[("hooks_outcome", "HooksOutcome"), ("hearth_path", "PathBuf")],
            |ctx, params| {
                let forbidden = params.get_string(0).ok_or("Expected actor")?.to_string();
                let actual = created_track_actor(&ctx)?;
                if actual == forbidden {
                    Err(format!(
                        "Self-asserted actor '{forbidden}' was persisted as the principal — laundering gap"
                    ))
                } else {
                    Ok(())
                }
            },
        ),
        // A refused call must leave NOTHING behind. An assertion that only
        // checked the exit code would still pass if the engine refused the
        // response but had already written the artifact.
        check_def(
            "anvil-hooks created no track",
            &[("hooks_outcome", "HooksOutcome"), ("hearth_path", "PathBuf")],
            |ctx, _params| {
                let o = outcome(&ctx)?;
                if !o.created_track.is_empty() {
                    return Err(format!(
                        "Refused call still reported creating '{}'",
                        o.created_track
                    ));
                }
                let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
                let tracks = hearth.join("tracks");
                let created: Vec<String> = std::fs::read_dir(&tracks)
                    .map(|entries| {
                        entries
                            .flatten()
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .filter(|n| n.ends_with("_credentialed_channel"))
                            .collect()
                    })
                    .unwrap_or_default();
                if created.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "Refused call still persisted track(s): {}",
                        created.join(", ")
                    ))
                }
            },
        ),
    ]
}

/// Read the persisted transition actor of the track `begin` reported creating.
fn created_track_actor(ctx: &Context) -> Result<String, String> {
    let o = outcome(&ctx)?;
    if o.created_track.is_empty() {
        return Err(format!(
            "anvil-hooks reported no created track, so there is no persisted actor to read:\n{}",
            o.output
        ));
    }
    let hearth = ctx.get::<PathBuf>("hearth_path").ok_or("No hearth_path")?;
    anvil_test_support::engine::last_transition_actor_at(hearth, &o.created_track)
}
