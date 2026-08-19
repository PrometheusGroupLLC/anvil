use crate::domain::shared_types::ActorIdentity;
use crate::ports::actor_write_port::ActorWriteError;

/// How a `status.yaml` that cannot carry an actor upsert is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MalformedPolicy {
    /// Historic non-K8 behavior: an unparseable document takes the
    /// add-if-absent leg and the line writer creates a fresh section.
    Lenient,
    /// K8 behavior (plan Task 4): a malformed existing `status.yaml` is an
    /// ERROR. Treating it as empty would silently drop actor metadata into a
    /// journal that then claims to hold the exact desired bytes. "Malformed"
    /// here is BOTH unparseable YAML and a parseable document whose `actors`
    /// key holds something an actor block cannot be added to — the add-if-absent
    /// leg discards that value without a word.
    Strict,
}

/// The PURE renderer behind
/// [`ActorWritePort::upsert_actor_configuration`](crate::ports::actor_write_port::ActorWritePort::upsert_actor_configuration).
///
/// Returns `Ok(Some(bytes))` with the exact desired `status.yaml` content, or
/// `Ok(None)` for the match-no-op leg (the live bytes are already correct).
/// This is the seam a K8 preparation uses to put exact rendered bytes into its
/// journal manifest BEFORE the transaction is marked `applying`.
pub fn render_upserted_actor_configuration(
    existing_status_bytes: &str,
    identity: &ActorIdentity,
    policy: MalformedPolicy,
) -> Result<Option<String>, String> {
    if policy == MalformedPolicy::Strict {
        let parsed = serde_yaml::from_str::<serde_yaml::Value>(existing_status_bytes)
            .map_err(|e| format!("existing status.yaml is not parseable YAML: {e}"))?;
        // A present-but-unusable `actors` key is the OTHER way this document is
        // malformed: `classify_leg` would take the add-if-absent leg, and
        // `add_actor_block` rewrites the `actors:` line, SILENTLY DROPPING
        // whatever it held. Under the K8 policy that is an error, never a
        // fallback — otherwise the journal claims to hold the exact desired
        // bytes while the actor metadata it replaced is gone.
        match parsed.get("actors") {
            None | Some(serde_yaml::Value::Null) | Some(serde_yaml::Value::Mapping(_)) => {}
            Some(other) => {
                return Err(format!(
                    "existing status.yaml holds an `actors` key that is not a mapping \
                     ({other:?}); upserting the actor would silently drop it"
                ))
            }
        }
    }
    let leg = classify_leg(existing_status_bytes, identity)?;
    match leg {
        Leg::AddIfAbsent => Ok(Some(add_actor_block(existing_status_bytes, identity))),
        Leg::MatchNoOp => Ok(None),
        Leg::MismatchAppend => append_configuration_entry(existing_status_bytes, identity)
            .map(Some)
            .map_err(|e| e.to_string()),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Leg {
    AddIfAbsent,
    MatchNoOp,
    MismatchAppend,
}

/// Inspect the parsed actors table to decide which leg of the rule
/// applies. Humans always take the AddIfAbsent or MatchNoOp leg —
/// they have no `configurations` to compare.
fn classify_leg(content: &str, identity: &ActorIdentity) -> Result<Leg, String> {
    // serde_yaml parsing is best-effort; we only need to interrogate
    // actors[name]. If the doc is malformed or actors is missing,
    // treat as add-if-absent — the line-based writer creates a fresh
    // section.
    let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(content);
    let value = match parsed {
        Ok(v) => v,
        Err(_) => return Ok(Leg::AddIfAbsent),
    };
    let actors = match value.get("actors") {
        Some(serde_yaml::Value::Mapping(m)) => m,
        _ => return Ok(Leg::AddIfAbsent),
    };
    let actor_value = match actors.get(serde_yaml::Value::String(identity.name.clone())) {
        Some(v) => v,
        None => return Ok(Leg::AddIfAbsent),
    };

    if identity.actor_type == "human" {
        // Humans have no configurations list — presence is enough.
        return Ok(Leg::MatchNoOp);
    }

    let configs = match actor_value.get("configurations") {
        Some(serde_yaml::Value::Sequence(s)) => s,
        _ => {
            return Err(format!(
                "actor '{}' has no configurations list",
                identity.name
            ))
        }
    };
    let latest = match configs.last() {
        Some(v) => v,
        None => return Ok(Leg::MismatchAppend),
    };

    if latest_matches_identity(latest, identity) {
        Ok(Leg::MatchNoOp)
    } else {
        Ok(Leg::MismatchAppend)
    }
}

fn latest_matches_identity(entry: &serde_yaml::Value, identity: &ActorIdentity) -> bool {
    let model = entry.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let provider = entry.get("provider").and_then(|v| v.as_str()).unwrap_or("");
    let details = entry.get("details");
    let context_window = details
        .and_then(|d| d.get("context_window"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let sdk_version = details
        .and_then(|d| d.get("sdk_version"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let entrypoint = details
        .and_then(|d| d.get("entrypoint"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    model == identity.model
        && provider == identity.provider
        && context_window == identity.context_window
        && sdk_version == identity.sdk_version
        && entrypoint == identity.entrypoint
}

/// Add-if-absent leg: insert a fresh actor block under `actors:` (or
/// create the section if missing). Mirrors the previous behavior of
/// `fs_begin_adapter::seed_actor` and `fs_snapshot_adapter::seed_actor`.
fn add_actor_block(content: &str, identity: &ActorIdentity) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let block = build_actor_block(identity);

    let actors_line_idx = lines.iter().position(|l| is_actors_header(l));
    match actors_line_idx {
        Some(actors_idx) => {
            // Normalize inline-empty shapes (`actors: {}` / `actors: null`)
            // into block style before inserting.
            let value_after = lines[actors_idx]
                .trim_start()
                .strip_prefix("actors:")
                .map(|s| s.trim())
                .unwrap_or("");
            if !value_after.is_empty() {
                lines[actors_idx] = "actors:".to_string();
            }
            let mut insert_at = lines.len();
            for j in (actors_idx + 1)..lines.len() {
                if !lines[j].starts_with(' ') && !lines[j].is_empty() {
                    insert_at = j;
                    break;
                }
            }
            for (k, line) in block.into_iter().enumerate() {
                lines.insert(insert_at + k, line);
            }
        }
        None => {
            // No actors table present. Insert one just before
            // `transitions:` (or at the end of file).
            let insert_at = lines
                .iter()
                .position(|l| l.trim_start().starts_with("transitions:"))
                .unwrap_or(lines.len());
            lines.insert(insert_at, "actors:".to_string());
            for (k, line) in block.into_iter().enumerate() {
                lines.insert(insert_at + 1 + k, line);
            }
        }
    }

    ensure_trailing_newline(lines.join("\n"))
}

/// Mismatch-append leg: append a new configuration entry to the
/// existing actor's configurations list. Existing entries are
/// preserved unchanged.
fn append_configuration_entry(
    content: &str,
    identity: &ActorIdentity,
) -> Result<String, ActorWriteError> {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Find the actor's name line (e.g., "  Sarigue-938624:").
    let actor_marker = format!("  {}:", identity.name);
    let name_idx = lines
        .iter()
        .position(|l| l.trim_end() == actor_marker.trim_end())
        .ok_or_else(|| ActorWriteError::MalformedStatus {
            artifact_path: String::new(),
            message: format!(
                "actor '{}' present in parsed YAML but its `  {}:` line was not found",
                identity.name, identity.name
            ),
        })?;

    // Find `    configurations:` within this actor's block (must come
    // before any line returning to a shallower indent).
    let mut configs_idx = None;
    for j in (name_idx + 1)..lines.len() {
        let line = &lines[j];
        if line.is_empty() {
            continue;
        }
        if !line.starts_with("    ") {
            // Left the actor's block without finding configurations.
            break;
        }
        if line.trim_end() == "    configurations:" {
            configs_idx = Some(j);
            break;
        }
    }
    let configs_idx = configs_idx.ok_or_else(|| ActorWriteError::MalformedStatus {
        artifact_path: String::new(),
        message: format!(
            "actor '{}' has no `    configurations:` line under its block",
            identity.name
        ),
    })?;

    // Find the insertion point: just before the next line that's NOT
    // indented to the configurations-entry depth (>= 6 spaces). This
    // keeps the new entry as the last sibling under `configurations:`.
    let mut insert_at = lines.len();
    for j in (configs_idx + 1)..lines.len() {
        let line = &lines[j];
        if line.is_empty() {
            continue;
        }
        if !line.starts_with("      ") {
            insert_at = j;
            break;
        }
    }

    let new_entry = build_configuration_entry(identity);
    for (k, line) in new_entry.into_iter().enumerate() {
        lines.insert(insert_at + k, line);
    }

    Ok(ensure_trailing_newline(lines.join("\n")))
}

fn build_actor_block(identity: &ActorIdentity) -> Vec<String> {
    let mut block = Vec::new();
    block.push(format!("  {}:", identity.name));
    if identity.actor_type == "human" {
        block.push("    type: human".to_string());
    } else {
        block.push(format!("    type: {}", identity.actor_type));
        block.push("    configurations:".to_string());
        block.extend(build_configuration_entry(identity));
    }
    block
}

fn build_configuration_entry(identity: &ActorIdentity) -> Vec<String> {
    vec![
        format!("      - at: \"{}\"", identity.registered_at),
        format!("        model: {}", identity.model),
        format!("        provider: {}", identity.provider),
        "        details:".to_string(),
        format!("          context_window: {}", identity.context_window),
        format!("          sdk_version: \"{}\"", identity.sdk_version),
        format!("          entrypoint: {}", identity.entrypoint),
    ]
}

/// `actors:` line in any legal YAML shape (block-style header, inline
/// empty mapping `actors: {}`, or explicit null `actors: null`/`~`).
fn is_actors_header(line: &str) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("actors:") {
        return false;
    }
    // Reject lines like `actors_foo:` etc.
    let after = &trimmed[7..]; // length of "actors:" is 7
    if after.is_empty() {
        return true;
    }
    let first = after.chars().next().unwrap();
    matches!(first, ' ' | '\t')
}

fn ensure_trailing_newline(mut s: String) -> String {
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}
