use brine_core::catalog_extract::ExtractedStep;
use brine_runner_rust::registry::{Registry, StepDef};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub fn run_main(domain_steps: Vec<(&'static str, Vec<StepDef>)>) {
    run_main_with_default_features(domain_steps, default_features());
}

pub fn run_main_with_default_features(
    domain_steps: Vec<(&'static str, Vec<StepDef>)>,
    default_feature_patterns: Vec<PathBuf>,
) {
    let args: Vec<String> = std::env::args().collect();

    if has_flag(&args, "--catalog") {
        catalog_mode(domain_steps);
    } else if has_flag(&args, "--check") {
        let patterns = extract_features_arg(&args).unwrap_or_else(|| {
            eprintln!("--check requires --features <glob|path>...");
            std::process::exit(1);
        });
        check_mode(&patterns, domain_steps);
    } else {
        run_mode(&args, domain_steps, default_feature_patterns);
    }
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn extract_features_arg(args: &[String]) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--features" {
            i += 1;
            while i < args.len() && !args[i].starts_with("--") {
                result.push(args[i].clone());
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn extract_tags_arg(args: &[String], flag: &str) -> Vec<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            i += 1;
            if i < args.len() {
                return args[i].split(',').map(|s| s.to_string()).collect();
            }
        }
        i += 1;
    }
    Vec::new()
}

fn extract_line_arg(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--line" {
            i += 1;
            if i < args.len() {
                return args[i].parse().ok();
            }
        }
        i += 1;
    }
    None
}

fn resolve_glob(pattern: &str) -> Vec<PathBuf> {
    let mut results: Vec<PathBuf> = glob::glob(pattern)
        .expect("Invalid glob pattern")
        .filter_map(|entry| entry.ok())
        .collect();
    results.sort();
    results
}

fn resolve_features(patterns: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for pattern in patterns {
        if pattern.contains('*') || pattern.contains('?') {
            files.extend(resolve_glob(pattern));
        } else {
            files.push(PathBuf::from(pattern));
        }
    }
    files
}

fn scenario_contains_line(scenario: &brine_core::parser::Scenario, line: usize) -> bool {
    if scenario.line == line {
        return true;
    }
    if let Some(last_step) = scenario.steps.last() {
        return line > scenario.line && line <= last_step.line;
    }
    false
}

fn build_registry(domain_steps: Vec<(&'static str, Vec<StepDef>)>) -> Registry {
    Registry::from_defs(domain_steps.into_iter().flat_map(|(_, s)| s))
}

fn catalog_mode(domain_steps: Vec<(&'static str, Vec<StepDef>)>) {
    for (source_file, defs) in domain_steps {
        for def in defs {
            let entry = ExtractedStep {
                pattern: def.pattern,
                mode: def.mode,
                requires: def.requires,
                provides: def.provides,
                source_file: source_file.to_string(),
                seam: def.seam,
                contract_provides: def.contract_provides,
                contract_requires: def.contract_requires,
            };
            let json = serde_json::to_string(&entry).expect("Failed to serialize catalog entry");
            println!("{}", json);
        }
    }
}

fn check_mode(patterns: &[String], domain_steps: Vec<(&'static str, Vec<StepDef>)>) {
    let registry = build_registry(domain_steps);
    let files = resolve_features(patterns);
    let path_refs: Vec<&std::path::Path> = files.iter().map(|p| p.as_path()).collect();
    let mut reporter = brine_core::events::JsonReporter::new(std::io::stdout());
    match brine_runner_rust::check::check_files(&path_refs, &registry, &[], &mut reporter) {
        Ok(result) => {
            let _ = reporter.flush();
            if !result.success {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_mode(
    args: &[String],
    domain_steps: Vec<(&'static str, Vec<StepDef>)>,
    default_feature_patterns: Vec<PathBuf>,
) {
    let include_tags = extract_tags_arg(args, "--tags");
    let exclude_tags = extract_tags_arg(args, "--exclude-tags");
    let filter = if !include_tags.is_empty() || !exclude_tags.is_empty() {
        Some(brine_runner_rust::harness::TagFilter {
            include: include_tags,
            exclude: exclude_tags,
        })
    } else {
        None
    };

    let line_filter = extract_line_arg(args);

    let files = match extract_features_arg(args) {
        Some(patterns) => resolve_features(&patterns),
        None => default_feature_patterns,
    };

    let registry = build_registry(domain_steps);

    let mut all_features = Vec::new();
    for file in &files {
        let mut parse_result = brine_core::parser::parse_file(file)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", file.display(), e));

        if let Some(target_line) = line_filter {
            for feature in &mut parse_result.features {
                feature
                    .scenarios
                    .retain(|s| scenario_contains_line(s, target_line));
            }
        }

        all_features.extend(parse_result.features);
    }

    if let Some(target_line) = line_filter {
        if all_features.iter().all(|f| f.scenarios.is_empty()) {
            eprintln!("No scenario found at line {}", target_line);
            std::process::exit(1);
        }
    }

    let combined = brine_core::parser::ParseResult {
        features: all_features,
        source_file: None,
    };

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let mut reporter = brine_core::events::JsonReporter::new(std::io::stdout());
        let mut options = brine_runner_rust::harness::RunOptions {
            seed: None,
            filter: filter.as_ref(),
            resources: None,
        };

        let result = brine_runner_rust::harness::run_parsed_with_options(
            &combined,
            &registry,
            &mut reporter,
            &mut options,
        )
        .await;

        let _ = reporter.flush();

        if result.total_failed > 0 {
            eprintln!(
                "{} of {} scenarios failed",
                result.total_failed, result.total_scenarios
            );
            std::process::exit(1);
        }
    });
}

pub fn default_features_from_workspace_patterns(patterns: &[&str]) -> Vec<PathBuf> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().expect("workspace root");
    let mut all = Vec::new();
    for pattern in patterns {
        let full = workspace_root.join(pattern);
        if let Some(s) = full.to_str() {
            all.extend(resolve_glob(s));
        }
    }
    all
}

/// Default feature discovery for direct cargo test invocation.
/// When invoked via brine run, features come from --features args instead.
fn default_features() -> Vec<PathBuf> {
    // Feature discovery anchored to the calling test binary's manifest dir.
    // Walk up to workspace root so per-crate test binaries can still find
    // all features when invoked directly via `cargo test`.
    let patterns = [
        "features/**/*.feature",
        "anvil-core/features/**/*.feature",
        "anvil-engine/features/**/*.feature",
        "anvil-mcp/features/**/*.feature",
    ];
    default_features_from_workspace_patterns(&patterns)
}

/// Ensure a workspace binary exists on disk, building it if necessary.
///
/// Contract:
/// 1. Target directory resolution: `$CARGO_TARGET_DIR` if set and non-empty;
///    else `<workspace_root>/target` (workspace root located by walking up
///    from `env!("CARGO_MANIFEST_DIR")` until a `Cargo.toml` with
///    `[workspace]` is found); else `<CARGO_MANIFEST_DIR>/target`.
///    Hardcoded relative paths are forbidden — they break under
///    `CARGO_TARGET_DIR` overrides.
/// 2. Within-process serialization via `OnceLock<Mutex<HashSet<String>>>`
///    so concurrent calls on the same binary don't race. Cargo's own file
///    lock covers cross-process races.
/// 3. On build failure, panic with a message that includes the binary name,
///    the full cargo command, and the captured stderr. Uses
///    `Command::output()` (not `.status()`) so stderr reaches the panic.
/// 4. Built with the SHIPPED feature set (see [`shipped_build_args`]), and
///    cargo is invoked once per binary per process rather than skipped when
///    the path exists. See the note below on why both halves are required.
///
/// # Why this builds with features and no longer trusts an existing path
///
/// This used to run `cargo build --bin <name>` — no features — and return early
/// whenever the output path existed. `scripts/build-kit.sh` ships every binary
/// with `--features anvil-engine/foundry-session`. So the suite tested a binary
/// unlike the one that ships, which is the same class of defect as the bug that
/// prompted this: `anvil-hooks` had no credential path at all, and nobody saw
/// it because verification was compiled out.
///
/// Concretely, with the feature off, precedence 2 of `kit_bearer::resolve`
/// (mint a ticket from the broker) is `#[cfg]`'d out and replaced by a refusal.
/// Every credential scenario therefore ran against a binary that *cannot* mint,
/// and passed — because "it was refused" is satisfied by either cause, no
/// credential or no broker client. `anvil_hooks_foundry_credential.feature`
/// now pins the discriminator: only a build with the broker client names
/// `FOUNDRY_BROKER_SOCKET` in its refusal.
///
/// The early return had to go too, and it is the subtler half. The doc above
/// said "staleness is cargo's concern" while bypassing cargo entirely — so a
/// path left behind by a plain `cargo build` would be reused verbatim, feature
/// set and all. Adding features while keeping the early return would fix
/// nothing on any machine that had already built once. Cargo is cheap when
/// up-to-date and is the only thing that actually knows whether the artifact
/// matches the requested features, so it is now always asked.
pub fn ensure_binary(name: &str) {
    static BUILT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let guard = BUILT.get_or_init(|| Mutex::new(HashSet::new()));
    let mut built = guard.lock().expect("ensure_binary mutex poisoned");

    // Memoize on the BINARY, not on the path existing: within one test process
    // the feature set is fixed, so one cargo invocation settles it. Across
    // processes cargo's own file lock and freshness check apply.
    if built.contains(name) {
        return;
    }

    let args = shipped_build_args(name);
    let output = std::process::Command::new("cargo")
        .args(&args)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "ensure_binary({name}): failed to invoke `cargo {}`: {e}",
                args.join(" ")
            )
        });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "ensure_binary({name}): `cargo {}` failed with status {:?}\n--- stderr ---\n{}",
            args.join(" "),
            output.status.code(),
            stderr
        );
    }

    built.insert(name.to_string());
}

/// The cargo invocation that reproduces how `scripts/build-kit.sh` ships `name`.
///
/// Feature selection is per-PACKAGE, not per-binary, and `--features` is
/// rejected in the root of a virtual workspace — hence the explicit `-p`. The
/// shipped line is:
///
/// ```text
/// cargo build --release -p anvil-mcp -p anvil-engine --features anvil-engine/foundry-session
/// ```
///
/// An unknown binary is a hard error rather than a no-feature default: a silent
/// default is exactly how the divergence this function exists to prevent got
/// in. A new binary must state what it ships with.
fn shipped_build_args(name: &str) -> Vec<String> {
    let (package, features) = match name {
        // Both live in the `anvil-engine` package; the feature gates the
        // production Foundry session verifier and the broker client.
        "anvil-engine" | "anvil-hooks" => ("anvil-engine", Some("anvil-engine/foundry-session")),
        // Ships alongside anvil-engine with the same feature enabled, which
        // reaches it through its path dependency on anvil-engine.
        "anvil-mcp" => ("anvil-mcp", Some("anvil-engine/foundry-session")),
        other => panic!(
            "ensure_binary({other}): no shipped feature set recorded for this binary. \
             Add it to `shipped_build_args` naming what `scripts/build-kit.sh` ships it with. \
             Defaulting to no features is what let the suite test a binary unlike the one \
             that ships."
        ),
    };

    let mut args = vec![
        "build".to_string(),
        "-p".to_string(),
        package.to_string(),
        "--bin".to_string(),
        name.to_string(),
    ];
    if let Some(f) = features {
        args.push("--features".to_string());
        args.push(f.to_string());
    }
    args
}

/// Resolve the absolute path of a workspace binary in `target/debug/`,
/// honouring `CARGO_TARGET_DIR` and walking up to the workspace root.
///
/// Step modules that spawn `anvil-engine` / `anvil-mcp` subprocesses must use
/// this helper — never hardcode `target/debug/<name>` relative to
/// `CARGO_MANIFEST_DIR`, because that path is wrong under `CARGO_TARGET_DIR`
/// overrides common in CI. `ensure_binary` (which builds the binary) and the
/// step-module spawn sites (which launch it) must agree on the location,
/// and this helper is the single source of truth for both.
pub fn binary_path(name: &str) -> PathBuf {
    resolve_target_dir().join("debug").join(name)
}

fn resolve_target_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Some(root) = find_workspace_root() {
        return root.join("target");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target")
}

fn find_workspace_root() -> Option<PathBuf> {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut current: &std::path::Path = &start;
    loop {
        let manifest = current.join("Cargo.toml");
        if manifest.exists() {
            if let Ok(contents) = std::fs::read_to_string(&manifest) {
                if contents.contains("[workspace]") {
                    return Some(current.to_path_buf());
                }
            }
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return None,
        }
    }
}
