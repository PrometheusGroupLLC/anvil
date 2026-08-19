use super::{
    available_artifact_types, is_terminal_state_for_kind, ArtifactType, CatalogResult,
    InvalidArtifact,
};
use crate::domain::playbook::loader::load_from_yaml;
use crate::domain::routing::{compute_execution_route, SUBJECT_FILTERED_ARTIFACT};
use crate::ports::hearth_reader::HearthReaderPort;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Application-level query handler for the catalog.
///
/// This is the query port boundary — the gRPC handler delegates to it,
/// keeping transport concerns separate from query assembly. The handler
/// takes a HearthReaderPort, calls list_artifacts(), filters terminal
/// states, and assembles the full catalog response with hardcoded
/// available types.
///
/// For playbook artifacts, the handler additionally reads each artifact's
/// `machine.yaml` (via `reader.read_playbook_machine_yaml`) and validates it.
/// Malformed artifacts appear in `invalid_artifacts` and are absent from
/// `active_artifacts`. One artifact's failure does not block others.
pub struct CatalogQueryHandler;

impl CatalogQueryHandler {
    pub fn execute(
        reader: &dyn HearthReaderPort,
    ) -> Result<CatalogResult, crate::domain::HearthError> {
        let all_artifacts = reader.list_artifacts()?;

        let mut active_artifacts = Vec::new();
        let mut invalid_artifacts: Vec<InvalidArtifact> = Vec::new();

        // Track loaded playbook kind values for duplicate-kind detection.
        // Maps kind -> first artifact_id that declared it.
        let mut kind_registry: HashMap<String, String> = HashMap::new();

        for artifact in all_artifacts {
            // Kind-aware: K8 finishes at done/superseded/aged_out; every other
            // kind keeps its existing cross-kind terminal answer.
            if is_terminal_state_for_kind(artifact.artifact_type.as_str(), &artifact.state) {
                continue;
            }

            // For playbook artifacts: load and validate machine.yaml.
            if artifact.artifact_type == ArtifactType::Playbook {
                match reader.read_playbook_machine_yaml(&artifact.id) {
                    Err(e) => {
                        // I/O error reading machine.yaml — treat as invalid.
                        let mut params = BTreeMap::new();
                        params.insert("artifact_id".to_string(), artifact.id.clone());
                        invalid_artifacts.push(InvalidArtifact {
                            id: artifact.id,
                            code: "playbook_yaml_parse_error".to_string(),
                            message: e.to_string(),
                            params,
                        });
                        continue;
                    }
                    Ok(None) => {
                        // No machine.yaml yet (artifact was created with begin but
                        // machine not yet authored). Surface as active without validation.
                        let mut a = artifact;
                        a.execution_route = compute_execution_route(
                            SUBJECT_FILTERED_ARTIFACT,
                            a.artifact_type.as_str(),
                            &a.state,
                            "",
                        );
                        active_artifacts.push(a);
                    }
                    Ok(Some(yaml_text)) => {
                        // Validate the machine.yaml content against its REAL hooks.
                        // Passing &[] here (the pre-2026-07-07 bug) declared every
                        // playbook hook-less, so any machine with a `hook:` reference
                        // was falsely flagged `playbook_unknown_hook_reference` in the
                        // catalog — every hook-bearing registered playbook looked
                        // invalid (e.g. kit_generation → intent.md, proposal_lifecycle
                        // → envision.md, both of which provably exist). The registration
                        // path (hearth_registry) already fetches the hook names via the
                        // same port method; the catalog must too. A hook-listing I/O
                        // failure degrades to the empty list (validation strictly no
                        // laxer than before) rather than aborting the whole catalog.
                        let hook_file_names = reader
                            .list_playbook_hooks(&artifact.id)
                            .unwrap_or_default();
                        match load_from_yaml(&artifact.id, &yaml_text, &hook_file_names) {
                            Err(e) => {
                                invalid_artifacts.push(e.to_invalid_artifact(&artifact.id));
                            }
                            Ok(machine) => {
                                // Duplicate-kind detection across all loaded playbook artifacts.
                                if let Some(first_id) = kind_registry.get(&machine.kind) {
                                    // Duplicate found: mark both as invalid.
                                    // The first artifact was already added to active_artifacts;
                                    // we need to retract it and add both to invalid_artifacts.
                                    active_artifacts.retain(|a| a.id != *first_id);
                                    let dup_error = crate::domain::playbook::load_error::PlaybookLoadError::DuplicateKindRegistration {
                                        artifact_ids: format!("{},{}", first_id, artifact.id),
                                        kind: machine.kind.clone(),
                                    };
                                    // First artifact entry.
                                    let mut params_first = BTreeMap::new();
                                    params_first.insert(
                                        "artifact_ids".to_string(),
                                        format!("{},{}", first_id, artifact.id),
                                    );
                                    params_first.insert("kind".to_string(), machine.kind.clone());
                                    // Check if the first artifact is already in invalid_artifacts
                                    // (in case it was already retracted from a previous duplicate).
                                    if !invalid_artifacts.iter().any(|iv| iv.id == *first_id) {
                                        invalid_artifacts.push(InvalidArtifact {
                                            id: first_id.clone(),
                                            code: "playbook_duplicate_kind_registration"
                                                .to_string(),
                                            message: dup_error.to_string(),
                                            params: params_first,
                                        });
                                    }
                                    // Second (current) artifact entry.
                                    let mut params_second = BTreeMap::new();
                                    params_second.insert(
                                        "artifact_ids".to_string(),
                                        format!("{},{}", first_id, artifact.id),
                                    );
                                    params_second.insert("kind".to_string(), machine.kind.clone());
                                    invalid_artifacts.push(InvalidArtifact {
                                        id: artifact.id.clone(),
                                        code: "playbook_duplicate_kind_registration".to_string(),
                                        message: dup_error.to_string(),
                                        params: params_second,
                                    });
                                } else {
                                    kind_registry.insert(machine.kind.clone(), artifact.id.clone());
                                    let mut a = artifact;
                                    a.execution_route = compute_execution_route(
                                        SUBJECT_FILTERED_ARTIFACT,
                                        a.artifact_type.as_str(),
                                        &a.state,
                                        "",
                                    );
                                    active_artifacts.push(a);
                                }
                            }
                        }
                    }
                }
            } else {
                active_artifacts.push(artifact);
            }
        }

        Ok(CatalogResult {
            active_artifacts,
            available_types: available_artifact_types(),
            invalid_artifacts,
        })
    }
}
