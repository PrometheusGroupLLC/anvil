// ---------------------------------------------------------------------------
// Manifest builders — the pure bridge from a governed preparation to the exact
// journaled bytes. Nothing here reads the filesystem: every expected old hash
// comes from the strictly loaded source, so a preparation is bound to exactly
// the revisions it validated.
// ---------------------------------------------------------------------------

use crate::domain::actor_configuration::{render_upserted_actor_configuration, MalformedPolicy};
use crate::domain::backlog_item::{
    self as bi, BacklogItem, DerivedRankAppend, DriverRole, HistoryEntry as HE,
    PreparedBacklogTransition,
};
use crate::domain::content_hash::content_hash;
use crate::domain::shared_types::ActorIdentity;
use crate::ports::backlog_item_port::{
    BacklogEffect, BacklogJournalManifest, BacklogStoreError, LoadedBacklogItem,
    PreparedBacklogCommit, RevisionExpectation, StagedFile, BACKLOG_ITEMS_DIR,
    BACKLOG_REGISTRY_FILE,
};
use crate::ports::transition_event_write_port::{event_file_name, TransitionRecord};

/// One registry row. The K8 registry is a whole-file deterministic projection,
/// so a partial edit can never leave it half-moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogRegistryRow {
    pub id: String,
    pub title: String,
    pub section: String,
}

/// The seven K8 registry sections, in lifecycle order.
pub const BACKLOG_REGISTRY_SECTIONS: [&str; 7] = [
    "candidate",
    "ready",
    "in_flight",
    "done",
    "parked",
    "superseded",
    "aged_out",
];

/// Render `backlog_items.md` deterministically from the complete row set.
pub fn render_backlog_registry(rows: &[BacklogRegistryRow]) -> String {
    let mut out = String::from("# Backlog Items\n");
    for section in BACKLOG_REGISTRY_SECTIONS {
        out.push_str(&format!("\n## {section}\n\n"));
        let mut in_section: Vec<&BacklogRegistryRow> =
            rows.iter().filter(|r| r.section == section).collect();
        in_section.sort_by(|a, b| a.id.cmp(&b.id));
        for row in in_section {
            out.push_str(&format!("- {} — {}\n", row.id, row.title));
        }
    }
    out
}

/// Serialize a history vector exactly the way the strict loader reads it back.
fn render_history(entries: &[HE]) -> Result<String, BacklogStoreError> {
    serde_yaml::to_string(entries).map_err(|e| BacklogStoreError::Invalid {
        message: format!("serialize history: {e}"),
    })
}

fn render_item(item: &BacklogItem) -> Result<String, BacklogStoreError> {
    serde_yaml::to_string(item).map_err(|e| BacklogStoreError::Invalid {
        message: format!("serialize item: {e}"),
    })
}

/// The derived rank append every position-changed item receives. Its role is
/// hard-coded `engine_auto` — a request can never select this authority
/// (§1.10) — while the initiating identity is retained as trigger attribution.
fn derived_rank_entry(
    derived: &DerivedRankAppend,
    actor: &ActorIdentity,
    at: &str,
    seq: u64,
    triggering_seq: Option<u64>,
) -> HE {
    let mut payload = serde_yaml::Mapping::new();
    payload.insert(
        serde_yaml::Value::String("position".to_string()),
        serde_yaml::Value::Number(derived.position.into()),
    );
    payload.insert(
        serde_yaml::Value::String("explanation".to_string()),
        serde_yaml::Value::String(derived.explanation.clone()),
    );
    if let Some(t) = triggering_seq {
        payload.insert(
            serde_yaml::Value::String("triggering_state_change_seq".to_string()),
            serde_yaml::Value::Number(t.into()),
        );
    }
    HE {
        seq,
        // Trigger attribution: the initiating identity is retained…
        actor: actor.name.clone(),
        // …but the AUTHORITY is server-fixed, never caller-selected.
        role: DriverRole::EngineAuto,
        at: at.to_string(),
        kind: crate::domain::backlog_item::HistoryKind::RankRecomputed,
        from_state: None,
        to_state: None,
        payload: Some(serde_yaml::Value::Mapping(payload)),
        note: None,
    }
}

/// Build the journaled compound write for one governed transition, including
/// every same-organ position rewritten by an advancement batch.
#[allow(clippy::too_many_arguments)]
pub fn build_transition_commit(
    operation_id: &str,
    prepared: &PreparedBacklogTransition,
    source: &LoadedBacklogItem,
    derived_sources: &[LoadedBacklogItem],
    registry_rows_after: &[BacklogRegistryRow],
    registry_old_hash: Option<String>,
    hi_res_prefix: &str,
    random_suffix: &str,
) -> Result<PreparedBacklogCommit, BacklogStoreError> {
    let mut effects = Vec::new();
    let mut expected = vec![
        RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "item".to_string(),
            hash: Some(source.item_hash.clone()),
        },
        RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "history".to_string(),
            hash: Some(source.history_hash.clone()),
        },
        RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "ledger".to_string(),
            hash: Some(source.ledger_hash.clone()),
        },
        RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "status".to_string(),
            hash: Some(source.status_hash.clone()),
        },
        RevisionExpectation {
            bi_id: String::new(),
            kind: "registry".to_string(),
            hash: registry_old_hash.clone(),
        },
    ];
    for other in derived_sources {
        expected.push(RevisionExpectation {
            bi_id: other.id.clone(),
            kind: "item".to_string(),
            hash: Some(other.item_hash.clone()),
        });
        expected.push(RevisionExpectation {
            bi_id: other.id.clone(),
            kind: "history".to_string(),
            hash: Some(other.history_hash.clone()),
        });
    }

    // ── the advancing target: status, item, history, preallocated event ─────
    //
    // status.yaml carries BOTH the actor upsert and the `state:` header. The
    // header is the same derived projection the generic
    // `SnapshotPort::append_transition` path maintains
    // (the `status_header` projection) — a K8 item advancing to `ready` must not be
    // the one artifact kind left declaring `state: candidate` forever. Here it
    // is strictly better than the generic path: both legs land in ONE journaled
    // effect against `expected_old`, so the header and the transition event
    // commit or roll back together — no half-updated file is reachable.
    let status_after_actor = render_upserted_actor_configuration(
        &source.status_bytes,
        &prepared.actor,
        MalformedPolicy::Strict,
    )
    .map_err(|message| BacklogStoreError::Malformed {
        path: format!("{}/status.yaml", source.relative_dir),
        message,
    })?;
    let status_desired = crate::domain::status_header::set_state_header(
        status_after_actor.as_deref().unwrap_or(&source.status_bytes),
        prepared.to.as_str(),
    );
    if status_desired != source.status_bytes {
        effects.push(BacklogEffect::Status {
            bi_id: source.id.clone(),
            path: format!("{}/status.yaml", source.relative_dir),
            expected_old: Some(source.status_hash.clone()),
            desired: status_desired,
        });
    }

    effects.push(BacklogEffect::Item {
        bi_id: source.id.clone(),
        path: format!("{}/item.yaml", source.relative_dir),
        expected_old: Some(source.item_hash.clone()),
        desired: render_item(&prepared.target_item)?,
    });

    // The printed-role `state_change` is ordered FIRST; the private derived
    // `engine_auto` rank append follows at the next sequence (§1.10).
    let mut target_history = source.history.clone();
    let state_change_seq = source.next_seq();
    let mut state_change = prepared.state_change.clone();
    state_change.seq = state_change_seq;
    target_history.push(state_change.clone());
    // Remembered so a SECOND append to the same file (the derived rank entry
    // below) expects exactly these intermediate bytes rather than absence.
    let state_change_bytes = render_history(&target_history)?;
    effects.push(BacklogEffect::History {
        bi_id: source.id.clone(),
        path: format!("{}/history.yaml", source.relative_dir),
        expected_seq: state_change_seq,
        expected_old: Some(source.history_hash.clone()),
        desired: state_change_bytes.clone(),
    });

    let record = TransitionRecord {
        to: prepared.to.as_str().to_string(),
        at: prepared.at.clone(),
        actor: prepared.actor.name.clone(),
        role: prepared.role.as_str().to_string(),
        approver: prepared.approver.clone(),
        note: prepared.note.clone(),
        satisfaction: None,
        event_type: None,
    };
    let file = event_file_name(hi_res_prefix, &record, random_suffix);
    effects.push(BacklogEffect::Event {
        bi_id: source.id.clone(),
        path: format!("{}/transitions/{file}", source.relative_dir),
        expected_old: None,
        desired: serde_yaml::to_string(&record).map_err(|e| BacklogStoreError::Invalid {
            message: format!("serialize transition event: {e}"),
        })?,
    });

    // ── derived position rewrites ───────────────────────────────────────────
    for derived in &prepared.derived_rank {
        if derived.bi_id == source.id {
            let entry = derived_rank_entry(
                derived,
                &prepared.actor,
                &prepared.at,
                state_change_seq + 1,
                Some(state_change_seq),
            );
            target_history.push(entry);
            effects.push(BacklogEffect::History {
                bi_id: source.id.clone(),
                path: format!("{}/history.yaml", source.relative_dir),
                expected_seq: state_change_seq + 1,
                expected_old: Some(content_hash(state_change_bytes.as_bytes())),
                desired: render_history(&target_history)?,
            });
            continue;
        }
        let other = derived_sources
            .iter()
            .find(|l| l.id == derived.bi_id)
            .ok_or_else(|| BacklogStoreError::Invalid {
                message: format!(
                    "the advancement batch rewrites '{}' but its strict source revision was \
                     not supplied",
                    derived.bi_id
                ),
            })?;
        let mut moved = other.item.clone();
        if let Some(rank) = moved.rank.as_mut() {
            rank.position = derived.position;
            rank.explanation = derived.explanation.clone();
        }
        effects.push(BacklogEffect::Item {
            bi_id: other.id.clone(),
            path: format!("{}/item.yaml", other.relative_dir),
            expected_old: Some(other.item_hash.clone()),
            desired: render_item(&moved)?,
        });
        let mut hist = other.history.clone();
        let seq = other.next_seq();
        hist.push(derived_rank_entry(
            derived,
            &prepared.actor,
            &prepared.at,
            seq,
            Some(state_change_seq),
        ));
        effects.push(BacklogEffect::History {
            bi_id: other.id.clone(),
            path: format!("{}/history.yaml", other.relative_dir),
            expected_seq: seq,
            expected_old: Some(other.history_hash.clone()),
            desired: render_history(&hist)?,
        });
    }

    // ── the single registry entry (CRITICAL for K8, never warn-and-continue) ─
    effects.push(BacklogEffect::Registry {
        path: BACKLOG_REGISTRY_FILE.to_string(),
        expected_old: registry_old_hash,
        desired: render_backlog_registry(registry_rows_after),
    });

    Ok(PreparedBacklogCommit {
        manifest: BacklogJournalManifest {
            operation_id: operation_id.to_string(),
            operation: format!("transition_{}", prepared.edge),
            actor: prepared.actor.clone(),
            effects,
            expected,
        },
        decide_commit: false,
    })
}

/// Build the journaled exact-ID genesis publication (plan Task 5).
#[allow(clippy::too_many_arguments)]
pub fn build_genesis_commit(
    operation_id: &str,
    item: &BacklogItem,
    created: &HE,
    actor: &ActorIdentity,
    status_bytes: &str,
    registry_rows_after: &[BacklogRegistryRow],
    registry_old_hash: Option<String>,
    hi_res_prefix: &str,
    random_suffix: &str,
) -> Result<PreparedBacklogCommit, BacklogStoreError> {
    let bi_id = item.backlog_item_id.clone();
    let record = TransitionRecord {
        to: item.state.as_str().to_string(),
        at: created.at.clone(),
        actor: actor.name.clone(),
        role: created.role.as_str().to_string(),
        approver: None,
        note: None,
        satisfaction: None,
        event_type: None,
    };
    let event = event_file_name(hi_res_prefix, &record, random_suffix);
    let files = vec![
        StagedFile {
            relative: "status.yaml".to_string(),
            contents: status_bytes.to_string(),
        },
        StagedFile {
            relative: "item.yaml".to_string(),
            contents: render_item(item)?,
        },
        StagedFile {
            relative: "history.yaml".to_string(),
            contents: render_history(std::slice::from_ref(created))?,
        },
        StagedFile {
            relative: format!("transitions/{event}"),
            contents: serde_yaml::to_string(&record).map_err(|e| BacklogStoreError::Invalid {
                message: format!("serialize genesis event: {e}"),
            })?,
        },
    ];
    let effects = vec![
        BacklogEffect::Publish {
            bi_id: bi_id.clone(),
            path: format!("{BACKLOG_ITEMS_DIR}/{bi_id}"),
            files,
        },
        BacklogEffect::Registry {
            path: BACKLOG_REGISTRY_FILE.to_string(),
            expected_old: registry_old_hash.clone(),
            desired: render_backlog_registry(registry_rows_after),
        },
    ];
    Ok(PreparedBacklogCommit {
        manifest: BacklogJournalManifest {
            operation_id: operation_id.to_string(),
            operation: "genesis".to_string(),
            actor: actor.clone(),
            effects,
            expected: vec![RevisionExpectation {
                bi_id: String::new(),
                kind: "registry".to_string(),
                hash: registry_old_hash,
            }],
        },
        decide_commit: false,
    })
}

/// Build the journaled compound write for one named post-genesis mutation
/// (plan Task 7).
///
/// The default ordered effect vector is, per affected item in byte-ID order:
/// actor status, item body, history append; then the single registry entry.
/// DECIDE commit is the one frozen special case: every `rank_committed`
/// authorization append is emitted BEFORE any approved position byte, so no
/// position file can ever precede its authorization entry — not even under a
/// crash between the two groups.
pub fn build_mutation_commit(
    operation_id: &str,
    prepared: &bi::PreparedBacklogMutation,
    sources: &[LoadedBacklogItem],
    registry_rows_after: &[BacklogRegistryRow],
    registry_old_hash: Option<String>,
) -> Result<PreparedBacklogCommit, BacklogStoreError> {
    let source_for = |bi_id: &str| -> Result<&LoadedBacklogItem, BacklogStoreError> {
        sources
            .iter()
            .find(|l| l.id == bi_id)
            .ok_or_else(|| BacklogStoreError::Invalid {
                message: format!(
                    "the mutation touches '{bi_id}' but its strict source revision was not \
                     supplied"
                ),
            })
    };

    let mut expected = vec![RevisionExpectation {
        bi_id: String::new(),
        kind: "registry".to_string(),
        hash: registry_old_hash.clone(),
    }];
    let mut touched: Vec<&str> = prepared
        .appends
        .iter()
        .map(|a| a.bi_id.as_str())
        .chain(prepared.items.iter().map(|i| i.bi_id.as_str()))
        .collect();
    touched.sort_unstable();
    touched.dedup();
    for bi_id in &touched {
        let source = source_for(bi_id)?;
        expected.push(RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "item".to_string(),
            hash: Some(source.item_hash.clone()),
        });
        expected.push(RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "history".to_string(),
            hash: Some(source.history_hash.clone()),
        });
        expected.push(RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "ledger".to_string(),
            hash: Some(source.ledger_hash.clone()),
        });
        expected.push(RevisionExpectation {
            bi_id: source.id.clone(),
            kind: "status".to_string(),
            hash: Some(source.status_hash.clone()),
        });
    }

    let mut status_effects: Vec<BacklogEffect> = Vec::new();
    let mut history_effects: Vec<BacklogEffect> = Vec::new();
    let mut item_effects: Vec<BacklogEffect> = Vec::new();

    for bi_id in &touched {
        let source = source_for(bi_id)?;
        let status_desired = render_upserted_actor_configuration(
            &source.status_bytes,
            &prepared.actor,
            MalformedPolicy::Strict,
        )
        .map_err(|message| BacklogStoreError::Malformed {
            path: format!("{}/status.yaml", source.relative_dir),
            message,
        })?;
        if let Some(desired) = status_desired {
            status_effects.push(BacklogEffect::Status {
                bi_id: source.id.clone(),
                path: format!("{}/status.yaml", source.relative_dir),
                expected_old: Some(source.status_hash.clone()),
                desired,
            });
        }
    }

    // Exactly one fixed-kind append per affected item per public named
    // operation, so a per-item history file is rewritten exactly once.
    let mut ordered_appends: Vec<&bi::PreparedHistoryAppend> = prepared.appends.iter().collect();
    ordered_appends.sort_by(|a, b| a.bi_id.cmp(&b.bi_id));
    for append in ordered_appends {
        let source = source_for(&append.bi_id)?;
        let seq = source.next_seq();
        let mut entry = append.entry.clone();
        entry.seq = seq;
        let mut history = source.history.clone();
        history.push(entry);
        history_effects.push(BacklogEffect::History {
            bi_id: source.id.clone(),
            path: format!("{}/history.yaml", source.relative_dir),
            expected_seq: seq,
            expected_old: Some(source.history_hash.clone()),
            desired: render_history(&history)?,
        });
    }

    let mut ordered_items: Vec<&bi::PreparedItemWrite> = prepared.items.iter().collect();
    ordered_items.sort_by(|a, b| a.bi_id.cmp(&b.bi_id));
    for write in ordered_items {
        let source = source_for(&write.bi_id)?;
        item_effects.push(BacklogEffect::Item {
            bi_id: source.id.clone(),
            path: format!("{}/item.yaml", source.relative_dir),
            expected_old: Some(source.item_hash.clone()),
            desired: render_item(&write.item)?,
        });
    }

    let mut effects: Vec<BacklogEffect> = Vec::new();
    if prepared.decide_commit {
        // Authorization strictly before position.
        effects.extend(status_effects);
        effects.extend(history_effects);
        effects.extend(item_effects);
    } else {
        effects.extend(status_effects);
        effects.extend(item_effects);
        effects.extend(history_effects);
    }
    effects.push(BacklogEffect::Registry {
        path: BACKLOG_REGISTRY_FILE.to_string(),
        expected_old: registry_old_hash,
        desired: render_backlog_registry(registry_rows_after),
    });

    Ok(PreparedBacklogCommit {
        manifest: BacklogJournalManifest {
            operation_id: operation_id.to_string(),
            operation: prepared.operation.to_string(),
            actor: prepared.actor.clone(),
            effects,
            expected,
        },
        decide_commit: prepared.decide_commit,
    })
}

/// Build the journaled compound write for one engine-auto evaluation batch
/// (plan Task 8).
///
/// Per affected item in byte-ID order: actor status, item body, then its ORDERED
/// history appends (a surviving re-rank that ages out records `rank_recomputed`
/// then `state_change`; an advancement records the printed-role `state_change`
/// then the private derived `rank_recomputed`), then the preallocated
/// authoritative transition event for any item that actually moved. The single
/// registry entry closes the batch, with every moved section already applied.
#[allow(clippy::too_many_arguments)]
pub fn build_evaluation_commit(
    operation_id: &str,
    prepared: &bi::PreparedBacklogEvaluation,
    sources: &[LoadedBacklogItem],
    registry_rows_after: &[BacklogRegistryRow],
    registry_old_hash: Option<String>,
    hi_res_prefix: &str,
    random_suffix: &str,
) -> Result<PreparedBacklogCommit, BacklogStoreError> {
    let mut effects = Vec::new();
    let mut expected = vec![RevisionExpectation {
        bi_id: String::new(),
        kind: "registry".to_string(),
        hash: registry_old_hash.clone(),
    }];

    for plan in &prepared.plans {
        let source = sources
            .iter()
            .find(|l| l.id == plan.bi_id)
            .ok_or_else(|| BacklogStoreError::Invalid {
                message: format!(
                    "the evaluation touches '{}' but its strict source revision was not supplied",
                    plan.bi_id
                ),
            })?;
        for kind in ["item", "history", "ledger", "status"] {
            expected.push(RevisionExpectation {
                bi_id: source.id.clone(),
                kind: kind.to_string(),
                hash: Some(match kind {
                    "item" => source.item_hash.clone(),
                    "history" => source.history_hash.clone(),
                    "ledger" => source.ledger_hash.clone(),
                    _ => source.status_hash.clone(),
                }),
            });
        }

        if let Some(desired) = render_upserted_actor_configuration(
            &source.status_bytes,
            &prepared.actor,
            MalformedPolicy::Strict,
        )
        .map_err(|message| BacklogStoreError::Malformed {
            path: format!("{}/status.yaml", source.relative_dir),
            message,
        })? {
            effects.push(BacklogEffect::Status {
                bi_id: source.id.clone(),
                path: format!("{}/status.yaml", source.relative_dir),
                expected_old: Some(source.status_hash.clone()),
                desired,
            });
        }

        effects.push(BacklogEffect::Item {
            bi_id: source.id.clone(),
            path: format!("{}/item.yaml", source.relative_dir),
            expected_old: Some(source.item_hash.clone()),
            desired: render_item(&plan.final_item)?,
        });

        // Ordered appends, each chained to the exact bytes it follows so a
        // crash BETWEEN two appends recovers idempotently.
        let mut history = source.history.clone();
        let mut previous_hash = Some(source.history_hash.clone());
        let mut seq = source.next_seq();
        for append in &plan.appends {
            let mut entry = append.clone();
            entry.seq = seq;
            history.push(entry);
            let desired = render_history(&history)?;
            effects.push(BacklogEffect::History {
                bi_id: source.id.clone(),
                path: format!("{}/history.yaml", source.relative_dir),
                expected_seq: seq,
                expected_old: previous_hash.clone(),
                desired: desired.clone(),
            });
            previous_hash = Some(content_hash(desired.as_bytes()));
            seq += 1;
        }

        if let Some(to) = plan.moved_to {
            let record = TransitionRecord {
                to: to.as_str().to_string(),
                at: prepared.at.clone(),
                actor: prepared.actor.name.clone(),
                role: plan.role.as_str().to_string(),
                approver: None,
                note: None,
                satisfaction: None,
                event_type: None,
            };
            let file = event_file_name(hi_res_prefix, &record, random_suffix);
            effects.push(BacklogEffect::Event {
                bi_id: source.id.clone(),
                path: format!("{}/transitions/{file}", source.relative_dir),
                expected_old: None,
                desired: serde_yaml::to_string(&record).map_err(|e| {
                    BacklogStoreError::Invalid {
                        message: format!("serialize transition event: {e}"),
                    }
                })?,
            });
        }
    }

    effects.push(BacklogEffect::Registry {
        path: BACKLOG_REGISTRY_FILE.to_string(),
        expected_old: registry_old_hash,
        desired: render_backlog_registry(registry_rows_after),
    });

    Ok(PreparedBacklogCommit {
        manifest: BacklogJournalManifest {
            operation_id: operation_id.to_string(),
            operation: "evaluate_backlog".to_string(),
            actor: prepared.actor.clone(),
            effects,
            expected,
        },
        decide_commit: false,
    })
}
