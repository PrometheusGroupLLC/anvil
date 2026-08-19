//! `anvil-hearth-probe` — the real Catalog/Describe answer over a real hearth.
//!
//! **Why this binary exists.** C-r.7a assertion 4a (reassigned from C-p.3 by
//! ERRATUM-5) requires that a definition which arrived *through the registration
//! path* is reported by anvil's **Catalog** as `artifact_type: playbook` and by
//! **Describe** as a `playbook`. Catalog and Describe are anvil-core domain
//! surfaces; Foundry declares no dependency on `anvil-core`, and the engine's
//! WS bridge exposes no `catalog` query. ERRATUM-6 rules that the honest way to
//! close that half is a probe binary that runs the REAL
//! `CatalogQueryHandler` and the REAL `DescribeQueryHandler` over a REAL hearth
//! directory and prints their answers as JSON.
//!
//! There is **no mock, no seeded fixture and no stand-in** here: the reader is
//! `FileSystemHearthReader`, the describe port is `FileSystemDescribeAdapter`,
//! and the registry is `SeedPlaybookRegistry` — exactly the composition the
//! engine's query handlers use.
//!
//! Usage:
//!
//! ```text
//! anvil-hearth-probe --hearth <path> [--describe <artifact_id>]
//! ```
//!
//! Exit codes: `0` on a produced answer (including a Describe error, which is
//! reported *in* the JSON), `2` on a usage error, `3` when the hearth itself
//! cannot be read. Nothing is ever degraded to an empty-but-successful answer.

use anvil_core::domain::catalog::CatalogQueryHandler;
use anvil_core::domain::describe::{DescribeQueryHandler, DescribeRequest, DescribeResult};
use anvil_core::domain::playbook::registry::SeedPlaybookRegistry;
use anvil_core_hearth::fs_describe_adapter::FileSystemDescribeAdapter;
use anvil_core_hearth::fs_hearth_reader::FileSystemHearthReader;
use serde_json::json;
use std::path::PathBuf;

fn main() {
    let mut hearth: Option<PathBuf> = None;
    let mut describe: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--hearth" => hearth = args.next().map(PathBuf::from),
            "--describe" => describe = args.next(),
            other => {
                eprintln!("anvil-hearth-probe: unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }
    let Some(hearth) = hearth else {
        eprintln!("anvil-hearth-probe: --hearth <path> is required");
        std::process::exit(2);
    };

    let reader = FileSystemHearthReader::new(hearth.clone());
    let catalog = match CatalogQueryHandler::execute(&reader) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("anvil-hearth-probe: catalog query failed: {e}");
            std::process::exit(3);
        }
    };

    let registry = SeedPlaybookRegistry;
    let describe_json = describe.map(|identifier| {
        let port = FileSystemDescribeAdapter::with_registry(hearth.clone(), &registry);
        match DescribeQueryHandler::execute(
            &port,
            &registry,
            DescribeRequest {
                identifier: identifier.clone(),
            },
        ) {
            Ok(DescribeResult::InstanceInfo {
                id,
                artifact_type,
                state,
                transition_count,
                ..
            }) => json!({
                "identifier": identifier,
                "resolved": "instance",
                "id": id,
                "artifact_type": artifact_type,
                "state": state,
                "transition_count": transition_count,
            }),
            Ok(DescribeResult::TypeInfo { name, .. }) => json!({
                "identifier": identifier,
                "resolved": "type",
                "name": name,
            }),
            Err(e) => json!({
                "identifier": identifier,
                "resolved": "error",
                "error": e.to_string(),
            }),
        }
    });

    let out = json!({
        "probe": "anvil-hearth-probe",
        "hearth": hearth.display().to_string(),
        "catalog": {
            "active_artifacts": catalog.active_artifacts,
            "invalid_artifacts": catalog.invalid_artifacts,
        },
        "describe": describe_json,
    });
    println!("{}", serde_json::to_string(&out).expect("probe json"));
}
