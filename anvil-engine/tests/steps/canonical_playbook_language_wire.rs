//! In-crate step definitions for `features/canonical_playbook_language_wire.feature`
//! (track `retire_the_workflow_term`, wire slice: anvil-engine + anvil-mcp).
//!
//! Every assertion here reads a REAL `anvil-engine` process over a REAL
//! temp hearth through a REAL WebSocket to `/ws`. The `/ws` bridge and the gRPC
//! service are the SAME fold on the SAME port, so the method names and response
//! keys observed here ARE the wire contract. Grepping `src/ws_bridge.rs` or
//! `proto/anvil.proto` is never acceptance evidence and is not done here.

use anvil_test_support::{check_def, retained_temp_dir, step_def, Context, RetainedTempDir, StepDef};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Every read method the bridge must answer, in canonical vocabulary, with the
/// params each one needs.
fn canonical_methods(hearth: &Path) -> Vec<(&'static str, Value)> {
    let h = hearth.display().to_string();
    vec![
        ("playbook_activity", json!({ "hearth_path": h })),
        ("playbook_step_volume", json!({ "hearth_path": h, "kind": "track" })),
        ("playbook_fidelity", json!({ "hearth_path": h })),
        ("actor_activity", json!({ "hearth_path": h })),
        ("usage_timeseries", json!({ "hearth_path": h })),
        ("activity_summary", json!({ "hearth_path": h })),
        ("hook_manifest", json!({ "hearth_path": h })),
        ("playbook_atlas", json!({ "hearth_path": h })),
        ("playbook_atlas_detail", json!({ "hearth_path": h, "kind": "track" })),
        ("live_instances", json!({ "hearth_path": h })),
    ]
}

/// The retired names. A client that sends one of these must get a
/// method-not-found error: no alias, no dual-emission, no dead surface.
const RETIRED_METHODS: &[&str] = &[
    "workflow_activity",
    "workflow_step_volume",
    "workflow_fidelity",
];

// ---------------------------------------------------------------------------
// Fixture: a real hearth, a real engine, a real WebSocket.
// ---------------------------------------------------------------------------

struct WireFixture {
    _hearth_dir: RetainedTempDir,
    hearth: PathBuf,
    engine: Child,
    port: u16,
}

impl Drop for WireFixture {
    fn drop(&mut self) {
        let _ = self.engine.kill();
        let _ = self.engine.wait();
    }
}

fn free_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind ephemeral port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {}", e))?
        .port();
    drop(listener);
    Ok(port)
}

fn wait_for_port(port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("anvil-engine has a parent directory")
        .to_path_buf()
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn build_hearth() -> Result<(RetainedTempDir, PathBuf), String> {
    let (dir, hearth) = retained_temp_dir("anvil-wire-canonical-")?;
    let write = |rel: &str, body: &str| -> Result<(), String> {
        let path = hearth.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", rel, e))?;
        }
        std::fs::write(&path, body).map_err(|e| format!("write {}: {}", rel, e))
    };

    write(".hearth", "path: .\n")?;
    write("tracks.md", "# Tracks\n")?;
    write("projections/execution.md", "# Execution\n")?;

    // The real shipped playbook definitions, so every fold reads the same
    // registry production reads.
    copy_dir_all(&repo_root().join("playbooks"), &hearth.join("playbooks"))
        .map_err(|e| format!("copy playbooks: {}", e))?;

    // One playbook run, begun and driven to a later state by a named actor, so
    // the activity / fidelity / actor folds all have something real to report.
    write(
        "tracks/20260601T0900_wire_canonical_run/status.yaml",
        "version: 1\nkind: track\nstate: spec\ntransitions:\n  - to: specifying\n    at: \"2026-06-01T09:00:00Z\"\n    actor: Wirewright-000001\n    role: doer\n  - to: spec\n    at: \"2026-06-01T09:30:00Z\"\n    actor: Wirewright-000001\n    role: doer\nactivity:\n  - kind: begin\n    actor: Wirewright-000001\n    state: specifying\n    at: \"2026-06-01T09:00:00Z\"\n    conversation_id: \"wire-canonical\"\n",
    )?;
    write(
        "tracks/20260601T0900_wire_canonical_run/spec.md",
        "# A playbook run on the wire\n",
    )?;

    Ok((dir, hearth))
}

fn start_fixture() -> Result<WireFixture, String> {
    let (dir, hearth) = build_hearth()?;
    let port = free_port()?;

    let engine_binary = anvil_test_support::harness::binary_path("anvil-engine");
    let engine = Command::new(&engine_binary)
        .env("ANVIL_RENDEZVOUS_DISABLE", "1")
        .env(
            "ANVIL_TELEMETRY_DIR",
            std::env::temp_dir().join("anvil-brine-telemetry"),
        )
        .arg("--hearth")
        .arg(&hearth)
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {}", engine_binary.display(), e))?;

    if !wait_for_port(port) {
        return Err(format!(
            "anvil-engine never accepted connections on 127.0.0.1:{}",
            port
        ));
    }

    Ok(WireFixture {
        _hearth_dir: dir,
        hearth,
        engine,
        port,
    })
}

// ---------------------------------------------------------------------------
// A minimal synchronous RFC-6455 client. The step registry here is
// synchronous, and the frames are single small text messages, so a hand-rolled
// client keeps this module free of an async runtime.
// ---------------------------------------------------------------------------

fn ws_call(port: u16, method: &str, params: &Value) -> Result<Value, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("connect {}: {}", port, e))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| format!("set read timeout: {}", e))?;

    // A fixed client key is fine: the server's accept value is not validated here.
    let handshake = format!(
        "GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        port
    );
    stream
        .write_all(handshake.as_bytes())
        .map_err(|e| format!("write handshake: {}", e))?;

    // Read until the end of the HTTP response headers.
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    while !header.ends_with(b"\r\n\r\n") {
        let n = stream
            .read(&mut byte)
            .map_err(|e| format!("read handshake response: {}", e))?;
        if n == 0 {
            return Err(format!(
                "engine closed the connection during the /ws handshake (read {} header bytes)",
                header.len()
            ));
        }
        header.push(byte[0]);
    }
    let header_text = String::from_utf8_lossy(&header).to_string();
    if !header_text.starts_with("HTTP/1.1 101") {
        return Err(format!("/ws did not upgrade: {}", header_text.trim()));
    }

    // The bridge refuses a request that does not name the surface making it, so
    // the probe names itself here rather than being served anonymously. The
    // caller's own params are preserved; only the surface is added.
    let mut named = params.clone();
    if let Some(obj) = named.as_object_mut() {
        obj.insert("surface".to_string(), json!("test-harness"));
    } else {
        named = json!({ "surface": "test-harness" });
    }
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": named,
    })
    .to_string();
    write_masked_text_frame(&mut stream, request.as_bytes())?;
    let payload = read_text_frame(&mut stream)?;
    serde_json::from_slice(&payload)
        .map_err(|e| format!("{} reply is not JSON: {} (raw {:?})", method, e, String::from_utf8_lossy(&payload)))
}

fn write_masked_text_frame(stream: &mut TcpStream, payload: &[u8]) -> Result<(), String> {
    let mut frame: Vec<u8> = vec![0x81]; // FIN + text opcode
    let len = payload.len();
    if len < 126 {
        frame.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    let mask = [0x3au8, 0x7f, 0x11, 0x5c];
    frame.extend_from_slice(&mask);
    for (i, b) in payload.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    stream
        .write_all(&frame)
        .map_err(|e| format!("write ws frame: {}", e))
}

fn read_exact(stream: &mut TcpStream, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    let mut filled = 0;
    while filled < n {
        let read = stream
            .read(&mut buf[filled..])
            .map_err(|e| format!("read ws frame: {}", e))?;
        if read == 0 {
            return Err("engine closed the /ws connection mid-frame".to_string());
        }
        filled += read;
    }
    Ok(buf)
}

fn read_text_frame(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    loop {
        let head = read_exact(stream, 2)?;
        let opcode = head[0] & 0x0f;
        let masked = head[1] & 0x80 != 0;
        let len = match head[1] & 0x7f {
            126 => {
                let ext = read_exact(stream, 2)?;
                u16::from_be_bytes([ext[0], ext[1]]) as usize
            }
            127 => {
                let ext = read_exact(stream, 8)?;
                u64::from_be_bytes([
                    ext[0], ext[1], ext[2], ext[3], ext[4], ext[5], ext[6], ext[7],
                ]) as usize
            }
            other => other as usize,
        };
        let mask = if masked {
            Some(read_exact(stream, 4)?)
        } else {
            None
        };
        let mut payload = read_exact(stream, len)?;
        if let Some(mask) = mask {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        match opcode {
            0x1 => return Ok(payload),
            0x8 => return Err("engine sent a ws close frame instead of a reply".to_string()),
            // ping/pong/continuation: keep reading for the text reply.
            _ => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

fn evidence<'a>(ctx: &'a Context) -> Result<&'a Value, String> {
    ctx.get::<Value>("wire_evidence")
        .ok_or_else(|| "no wire_evidence in context (the When step did not run)".to_string())
}

/// Every JSON object key reachable in `value`, with the dotted path that leads
/// to it.
fn collect_keys(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                out.push((format!("{}.{}", path, key), key.clone()));
                collect_keys(child, &format!("{}.{}", path, key), out);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                collect_keys(child, &format!("{}[{}]", path, i), out);
            }
        }
        _ => {}
    }
}

pub fn steps() -> Vec<StepDef> {
    vec![
        step_def(
            "a real engine is serving a hearth carrying a playbook run",
            &[],
            &[("wire_fixture", "WireFixture")],
            |_ctx, _params| {
                let fixture = start_fixture()?;
                let mut out = Context::new();
                out.set("wire_fixture", fixture);
                Ok(out)
            },
        ),
        step_def(
            "the client calls every read method on the engine's JSON-RPC bridge",
            &[("wire_fixture", "WireFixture")],
            &[("wire_evidence", "JsonValue")],
            |mut ctx, _params| {
                let fixture = ctx
                    .take::<WireFixture>("wire_fixture")
                    .ok_or("no wire_fixture in context (the Given step did not run)")?;

                let mut canonical = serde_json::Map::new();
                for (method, params) in canonical_methods(&fixture.hearth) {
                    let reply = ws_call(fixture.port, method, &params)?;
                    canonical.insert(method.to_string(), reply);
                }

                let mut retired = serde_json::Map::new();
                for method in RETIRED_METHODS {
                    let reply = ws_call(
                        fixture.port,
                        method,
                        &json!({ "hearth_path": fixture.hearth.display().to_string(), "kind": "track" }),
                    )?;
                    retired.insert(method.to_string(), reply);
                }

                let mut out = Context::new();
                out.set(
                    "wire_evidence",
                    json!({ "canonical": canonical, "retired": retired }),
                );
                Ok(out)
            },
        ),
        check_def(
            "every canonical read method answers",
            &[("wire_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;
                let canonical = evidence["canonical"]
                    .as_object()
                    .ok_or("no canonical replies recorded")?;
                let mut failures = Vec::new();
                for (method, reply) in canonical {
                    if reply.get("result").is_none() {
                        failures.push(format!("{} -> {}", method, reply));
                    }
                }
                if !failures.is_empty() {
                    return Err(format!(
                        "{} canonical read method(s) did not answer with a result:\n{}",
                        failures.len(),
                        failures.join("\n")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "the retired workflow-named methods are refused",
            &[("wire_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;
                let retired = evidence["retired"]
                    .as_object()
                    .ok_or("no retired-method replies recorded")?;
                let mut survivors = Vec::new();
                for (method, reply) in retired {
                    let code = reply["error"]["data"]["code"].as_str().unwrap_or("");
                    if code != "method_not_found" {
                        survivors.push(format!(
                            "`{}` is still answered (error.data.code = {:?}): {}",
                            method, code, reply
                        ));
                    }
                }
                if !survivors.is_empty() {
                    return Err(format!(
                        "{} retired method name(s) still live on the wire — an alias is two \
                         names for one thing:\n{}",
                        survivors.len(),
                        survivors.join("\n")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "no key anywhere in any response says workflow",
            &[("wire_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;
                let canonical = evidence["canonical"]
                    .as_object()
                    .ok_or("no canonical replies recorded")?;
                let mut offenders = Vec::new();
                for (method, reply) in canonical {
                    let mut keys = Vec::new();
                    collect_keys(reply, method, &mut keys);
                    for (path, key) in keys {
                        if key.to_lowercase().contains("workflow") {
                            offenders.push(path);
                        }
                    }
                }
                if !offenders.is_empty() {
                    return Err(format!(
                        "{} response key(s) on the engine's read surface still say workflow:\n{}",
                        offenders.len(),
                        offenders.join("\n")
                    ));
                }
                Ok(())
            },
        ),
        check_def(
            "each actor lists the artifact kinds it has begun under the canonical key",
            &[("wire_evidence", "JsonValue")],
            |ctx, _params| {
                let evidence = evidence(&ctx)?;
                let actors = evidence["canonical"]["actor_activity"]["result"]["actors"]
                    .as_array()
                    .ok_or_else(|| {
                        format!(
                            "actor_activity returned no actors array: {}",
                            evidence["canonical"]["actor_activity"]
                        )
                    })?;
                let seeded = actors
                    .iter()
                    .find(|a| a["actor"].as_str() == Some("Wirewright-000001"))
                    .ok_or_else(|| {
                        format!(
                            "the seeded actor did not appear in actor_activity: {:?}",
                            actors
                        )
                    })?;
                let kinds = seeded["artifact_kinds"].as_array().ok_or_else(|| {
                    format!(
                        "the seeded actor carries no `artifact_kinds` key: {}",
                        seeded
                    )
                })?;
                if !kinds.iter().any(|k| k.as_str() == Some("track")) {
                    return Err(format!(
                        "`artifact_kinds` does not carry the governed artifact kind the actor \
                         actually began: {}",
                        seeded
                    ));
                }
                Ok(())
            },
        ),
    ]
}
