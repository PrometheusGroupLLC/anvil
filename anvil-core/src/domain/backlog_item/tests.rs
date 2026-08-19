//! Pure unit/property tests for the K8 backlog-item value model (plan Task 2).
//!
//! These exercise closed parsing, semantic invariants, every matrix state, all
//! exact tuples and wrong tuples, normalization, comparator totality/determinism/
//! monotonicity, the done predicate, policy parsing, and id boundaries. They use
//! constructed values and are NOT substitutes for the real minted-then-mutated
//! edge scenarios owned by later tasks.

use super::*;

// --- fixtures ---------------------------------------------------------------

fn bi(body: &str) -> String {
    format!("bi_{body}")
}

fn temper_ref() -> EvidenceRef {
    EvidenceRef { kind: EvidenceKind::TemperMeasure, id: "tm_1".to_string() }
}

fn origin_no_predictor() -> OriginBinding {
    OriginBinding {
        value_gap_served: temper_ref(),
        minting_council_id: None,
        experiment_id: None,
        predicted_value: None,
    }
}

/// A minimal valid `candidate` item (no rank / bindings / exit).
fn candidate_item() -> BacklogItem {
    BacklogItem {
        backlog_item_id: format!("bi_{}", "a".repeat(4)),
        business_node_id: format!("bn_{}", "a".repeat(10)),
        title: "a title".to_string(),
        description: None,
        action_class: ActionClass::Dev,
        effort_class: None,
        state: State::Candidate,
        intake: Intake {
            edge: IntakeEdge::SparkTriage,
            evidence_refs: vec![EvidenceRef { kind: EvidenceKind::Spark, id: "sp_1".to_string() }],
        },
        rank: None,
        playbook_binding: None,
        origin_binding: origin_no_predictor(),
        execution_binding: None,
        outcome_binding: None,
        exit: None,
    }
}

fn rank_inputs(effort: EffortClass, dep: DependencyStatus, age: u32, mag: f64, weight: f64) -> RankInputs {
    RankInputs {
        value_gap_magnitude: ValueGapMagnitude { r#ref: temper_ref(), magnitude: mag },
        nick_weight: weight,
        dependency_readiness: DependencyReadiness { status: dep, blocker_refs: Vec::new() },
        age,
        effort_class: effort,
    }
}

fn full_rank(effort: EffortClass, dep: DependencyStatus, age: u32, mag: f64, weight: f64) -> Rank {
    Rank {
        position: 1,
        inputs: rank_inputs(effort, dep, age, mag, weight),
        explanation: "why #1".to_string(),
    }
}

fn playbook_bound() -> PlaybookBinding {
    PlaybookBinding {
        playbook_definition_id: Some("lp_x".to_string()),
        route_to_intake: false,
    }
}

fn exec_binding() -> ExecutionBinding {
    ExecutionBinding {
        track_id: "20260101T0000_x".to_string(),
        playbook_definition_id: "lp_x".to_string(),
        playbook_run_id: "wf_x".to_string(),
        run_id: format!("lr_{}", "a".repeat(26)),
    }
}

fn outcome_reading() -> OutcomeBinding {
    OutcomeBinding {
        success_measure_id: Some("sm_1".to_string()),
        tree_node: "root/x".to_string(),
        reading_status: ReadingStatus::Reading,
        nick_signoff: false,
    }
}

/// A fully-equipped item in `from` state satisfying every item-local guard,
/// with the staged exit set appropriately for the `to` state of edge `n`.
fn equipped_item(from: State, to: State, n: u8) -> BacklogItem {
    let mut item = candidate_item();
    item.state = from;
    item.effort_class = Some(EffortClass::M);
    item.playbook_binding = Some(playbook_bound());
    item.rank = Some(full_rank(EffortClass::M, DependencyStatus::Ready, 0, 1.0, 1.0));
    item.execution_binding = Some(exec_binding());
    item.outcome_binding = Some(outcome_reading());
    item.exit = match to {
        State::Parked => Some(Exit {
            kind: ExitKind::Parked,
            wake_condition: Some(WakeCondition {
                kind: WakeKind::Manual,
                r#ref: None,
                predicate: "manual".to_string(),
            }),
            superseded_by: None,
            aged_out_reason: None,
        }),
        State::Superseded => Some(Exit {
            kind: ExitKind::Superseded,
            wake_condition: None,
            superseded_by: Some(bi("beef")),
            aged_out_reason: None,
        }),
        State::AgedOut => {
            let reason = match n {
                4 => AgedOutReason::StaleNoReady,
                9 => AgedOutReason::StaleNoPickup,
                16 => AgedOutReason::WakeUnreachable,
                _ => AgedOutReason::StaleNoReady,
            };
            Some(Exit {
                kind: ExitKind::AgedOut,
                wake_condition: None,
                superseded_by: None,
                aged_out_reason: Some(reason),
            })
        }
        _ => None,
    };
    item
}

// --- serde: round-trip + deny_unknown_fields --------------------------------

#[test]
fn serde_round_trips_byte_stably() {
    let mut item = equipped_item(State::Ready, State::Ready, 0);
    item.state = State::Ready;
    item.exit = None;
    let yaml1 = serde_yaml::to_string(&item).unwrap();
    let back: BacklogItem = serde_yaml::from_str(&yaml1).unwrap();
    let yaml2 = serde_yaml::to_string(&back).unwrap();
    assert_eq!(yaml1, yaml2, "serialize -> deserialize -> serialize must be byte-stable");
    assert_eq!(item, back);
}

#[test]
fn serde_rejects_unknown_fields() {
    let yaml = "kind: temper_measure\nid: tm_1\nbogus: 1\n";
    let parsed: Result<EvidenceRef, _> = serde_yaml::from_str(yaml);
    assert!(parsed.is_err(), "deny_unknown_fields must reject unknown keys");
}

#[test]
fn serde_rejects_unknown_enum_variant() {
    let parsed: Result<ActionClass, _> = serde_yaml::from_str("marketing");
    assert!(parsed.is_err(), "closed enum must reject unknown variant");
}

// --- id grammar (R6) --------------------------------------------------------

#[test]
fn id_validators_enforce_prefix_charset_and_minimum() {
    assert!(validate_backlog_item_id(&format!("bi_{}", "a".repeat(4))).is_ok());
    assert!(validate_backlog_item_id("bi_abc").is_err(), "body < 4 rejects");
    assert!(validate_backlog_item_id("xx_abcd").is_err(), "wrong prefix rejects");
    assert!(validate_backlog_item_id("bi_aiaa").is_err(), "excluded letter i rejects");
    assert!(validate_backlog_item_id("bi_aLaa").is_err(), "uppercase rejects");

    assert!(validate_business_node_id(&format!("bn_{}", "a".repeat(10))).is_ok());
    assert!(validate_business_node_id(&format!("bn_{}", "a".repeat(9))).is_err());

    assert!(validate_run_id(&format!("lr_{}", "a".repeat(26))).is_ok());
    assert!(validate_run_id(&format!("lr_{}", "a".repeat(25))).is_err());
}

#[test]
fn mint_produces_valid_bi_with_26_char_body() {
    let id = mint_backlog_item_id();
    assert!(id.starts_with("bi_"));
    assert_eq!(id.len(), 3 + 26);
    assert!(validate_backlog_item_id(&id).is_ok());
}

// --- origin predictor invariant (three exact cases + rejections) ------------

#[test]
fn origin_predictor_three_valid_cases() {
    // both null + null prediction
    let mut item = candidate_item();
    assert!(validate_item_semantics(&item).is_ok());

    // council + finite prediction
    item.origin_binding.minting_council_id = Some("cn_1".to_string());
    item.origin_binding.predicted_value = Some(1.5);
    assert!(validate_item_semantics(&item).is_ok());

    // experiment + finite prediction
    item.origin_binding.minting_council_id = None;
    item.origin_binding.experiment_id = Some("ex_1".to_string());
    assert!(validate_item_semantics(&item).is_ok());
}

#[test]
fn origin_rejects_both_predictors() {
    let mut item = candidate_item();
    item.origin_binding.minting_council_id = Some("cn_1".to_string());
    item.origin_binding.experiment_id = Some("ex_1".to_string());
    item.origin_binding.predicted_value = Some(1.0);
    assert!(matches!(validate_item_semantics(&item), Err(BacklogItemError::SemanticViolation { .. })));
}

#[test]
fn origin_rejects_predictor_without_prediction_and_prediction_without_predictor() {
    let mut item = candidate_item();
    item.origin_binding.minting_council_id = Some("cn_1".to_string());
    item.origin_binding.predicted_value = None;
    assert!(validate_item_semantics(&item).is_err(), "predictor without prediction rejects");

    let mut item2 = candidate_item();
    item2.origin_binding.predicted_value = Some(2.0);
    assert!(validate_item_semantics(&item2).is_err(), "prediction without predictor rejects");
}

#[test]
fn value_gap_ref_must_be_temper_kind() {
    let mut item = candidate_item();
    item.origin_binding.value_gap_served = EvidenceRef { kind: EvidenceKind::Spark, id: "sp_1".to_string() };
    assert!(validate_item_semantics(&item).is_err());
}

#[test]
fn rank_effort_mirror_must_match() {
    let mut item = equipped_item(State::Ready, State::Ready, 0);
    item.exit = None;
    // mirror equal -> ok
    assert!(validate_item_semantics(&item).is_ok());
    // mirror drift -> reject
    item.effort_class = Some(EffortClass::L);
    assert!(validate_item_semantics(&item).is_err());
}

#[test]
fn null_playbook_requires_route_to_intake() {
    let mut item = candidate_item();
    item.playbook_binding = Some(PlaybookBinding { playbook_definition_id: None, route_to_intake: false });
    assert!(validate_item_semantics(&item).is_err());
    item.playbook_binding = Some(PlaybookBinding { playbook_definition_id: None, route_to_intake: true });
    assert!(validate_item_semantics(&item).is_ok());
}

#[test]
fn non_finite_rank_inputs_reject() {
    let mut item = equipped_item(State::Ready, State::Ready, 0);
    item.exit = None;
    item.rank.as_mut().unwrap().inputs.nick_weight = f64::NAN;
    assert!(validate_item_semantics(&item).is_err());
}

// --- required-by-state matrix (R7/R8) ---------------------------------------

#[test]
fn candidate_requires_only_the_always_required() {
    let item = candidate_item();
    assert!(validate_required_by_state(&item, State::Candidate).is_ok());
}

#[test]
fn empty_evidence_rejects_at_every_state() {
    let mut item = candidate_item();
    item.intake.evidence_refs.clear();
    assert!(validate_required_by_state(&item, State::Candidate).is_err());
}

#[test]
fn ready_requires_rank_effort_playbook() {
    let mut item = candidate_item();
    item.state = State::Ready;
    assert!(validate_required_by_state(&item, State::Ready).is_err(), "missing rank/effort/playbook");
    item.effort_class = Some(EffortClass::M);
    item.playbook_binding = Some(playbook_bound());
    item.rank = Some(full_rank(EffortClass::M, DependencyStatus::Ready, 0, 1.0, 1.0));
    assert!(validate_required_by_state(&item, State::Ready).is_ok());
}

#[test]
fn in_flight_requires_bindings() {
    let mut item = candidate_item();
    item.state = State::InFlight;
    item.effort_class = Some(EffortClass::M);
    item.playbook_binding = Some(playbook_bound());
    item.rank = Some(full_rank(EffortClass::M, DependencyStatus::Ready, 0, 1.0, 1.0));
    assert!(validate_required_by_state(&item, State::InFlight).is_err(), "missing bindings");
    item.execution_binding = Some(exec_binding());
    item.outcome_binding = Some(outcome_reading());
    assert!(validate_required_by_state(&item, State::InFlight).is_ok());
}

#[test]
fn exit_states_require_matching_exit_kind_and_subfield() {
    // parked requires exit.kind=parked + wake_condition
    let mut parked = candidate_item();
    parked.state = State::Parked;
    parked.effort_class = Some(EffortClass::M);
    parked.playbook_binding = Some(playbook_bound());
    assert!(validate_required_by_state(&parked, State::Parked).is_err());
    parked.exit = Some(Exit {
        kind: ExitKind::Parked,
        wake_condition: Some(WakeCondition { kind: WakeKind::Manual, r#ref: None, predicate: "m".into() }),
        superseded_by: None,
        aged_out_reason: None,
    });
    assert!(validate_required_by_state(&parked, State::Parked).is_ok());

    // wrong exit kind rejects
    parked.exit = Some(Exit { kind: ExitKind::Done, wake_condition: None, superseded_by: None, aged_out_reason: None });
    assert!(validate_required_by_state(&parked, State::Parked).is_err());
}

#[test]
fn superseded_does_not_require_bindings_but_requires_rank_and_target() {
    // §1.5: state-local validator tolerates absent bindings at superseded (R4/R5 case).
    let mut item = candidate_item();
    item.state = State::Superseded;
    item.effort_class = Some(EffortClass::M);
    item.playbook_binding = Some(playbook_bound());
    item.rank = Some(full_rank(EffortClass::M, DependencyStatus::Ready, 0, 1.0, 1.0));
    item.exit = Some(Exit {
        kind: ExitKind::Superseded,
        wake_condition: None,
        superseded_by: Some(bi("beef")),
        aged_out_reason: None,
    });
    assert!(validate_required_by_state(&item, State::Superseded).is_ok(), "no bindings required at superseded");
}

// --- transition table (R10/R11/R12) -----------------------------------------

#[test]
fn tuple_count_is_29() {
    assert_eq!(LEGAL_TRANSITION_TUPLE_COUNT, 29);
    assert_eq!(legal_transition_tuples().len(), 29);
}

#[test]
fn every_printed_tuple_is_admitted_when_guards_hold() {
    let edges: &[(u8, State, State, &[DriverRole])] = &[
        (1, State::Candidate, State::Ready, &[DriverRole::OrganLoop, DriverRole::Orchestrator]),
        (2, State::Candidate, State::Parked, &[DriverRole::NickShape, DriverRole::OrganLoop]),
        (3, State::Candidate, State::Superseded, &[DriverRole::NickShape, DriverRole::Orchestrator]),
        (4, State::Candidate, State::AgedOut, &[DriverRole::EngineAuto]),
        (5, State::Ready, State::InFlight, &[DriverRole::TrackDriver]),
        (6, State::Ready, State::Candidate, &[DriverRole::OrganLoop, DriverRole::Orchestrator, DriverRole::NickShape]),
        (7, State::Ready, State::Parked, &[DriverRole::NickShape, DriverRole::OrganLoop]),
        (8, State::Ready, State::Superseded, &[DriverRole::NickShape, DriverRole::Orchestrator]),
        (9, State::Ready, State::AgedOut, &[DriverRole::EngineAuto]),
        (10, State::InFlight, State::Done, &[DriverRole::EngineAuto, DriverRole::NickShape]),
        (11, State::InFlight, State::Parked, &[DriverRole::TrackDriver, DriverRole::NickShape]),
        (12, State::InFlight, State::Superseded, &[DriverRole::NickShape, DriverRole::Orchestrator]),
        (13, State::Parked, State::Candidate, &[DriverRole::EngineAuto, DriverRole::NickShape]),
        (14, State::Parked, State::Ready, &[DriverRole::EngineAuto, DriverRole::NickShape]),
        (15, State::Parked, State::Superseded, &[DriverRole::NickShape, DriverRole::Orchestrator]),
        (16, State::Parked, State::AgedOut, &[DriverRole::EngineAuto]),
    ];
    let mut count = 0;
    for (n, from, to, roles) in edges {
        for role in *roles {
            let item = equipped_item(*from, *to, *n);
            let res = validate_transition(&item, *from, *to, *role);
            assert!(res.is_ok(), "edge #{n} {:?}->{:?} role {:?} should admit: {res:?}", from, to, role);
            count += 1;
        }
    }
    assert_eq!(count, 29);
}

#[test]
fn wrong_role_rejected() {
    let item = equipped_item(State::Candidate, State::Ready, 1);
    // #1 admits organ_loop/orchestrator only.
    assert!(matches!(
        validate_transition(&item, State::Candidate, State::Ready, DriverRole::NickShape),
        Err(BacklogItemError::WrongRole { edge: 1, .. })
    ));
}

#[test]
fn off_table_pair_rejected() {
    let item = equipped_item(State::Candidate, State::InFlight, 0);
    assert!(matches!(
        validate_transition(&item, State::Candidate, State::InFlight, DriverRole::OrganLoop),
        Err(BacklogItemError::IllegalTransition { .. })
    ));
}

#[test]
fn terminal_states_have_no_outgoing() {
    for from in [State::Done, State::Superseded, State::AgedOut] {
        let mut item = candidate_item();
        item.state = from;
        assert!(matches!(
            validate_transition(&item, from, State::Candidate, DriverRole::NickShape),
            Err(BacklogItemError::TerminalNoOutgoing { .. })
        ));
    }
}

#[test]
fn state_mismatch_rejected() {
    let item = candidate_item(); // state = candidate
    assert!(matches!(
        validate_transition(&item, State::Ready, State::InFlight, DriverRole::TrackDriver),
        Err(BacklogItemError::StateMismatch { .. })
    ));
}

#[test]
fn guard_unsatisfied_rejected() {
    // candidate->ready with no rank fails the #1 guard.
    let mut item = candidate_item();
    item.rank = None;
    assert!(matches!(
        validate_transition(&item, State::Candidate, State::Ready, DriverRole::OrganLoop),
        Err(BacklogItemError::GuardUnsatisfied { edge: 1, .. })
    ));
    // in_flight->done without a done-rule-satisfying reading fails #10.
    let mut inflight = equipped_item(State::InFlight, State::Done, 10);
    inflight.outcome_binding.as_mut().unwrap().reading_status = ReadingStatus::Registered;
    // The refusal NAMES which half of the frozen predicate failed, so a caller
    // can never confuse "no stored reading" with "unsigned unmeasurable".
    let refusal = validate_transition(
        &inflight,
        State::InFlight,
        State::Done,
        DriverRole::EngineAuto,
    )
    .expect_err("a registered-only outcome may never satisfy the done rule");
    assert!(
        matches!(refusal, BacklogItemError::GuardUnsatisfied { edge: 10, .. }),
        "unexpected refusal: {refusal}"
    );
    assert!(
        refusal.to_string().contains("reading"),
        "the refusal must name the missing stored reading: {refusal}"
    );
    // #12 rejects when EITHER pickup binding is missing (R4/R5 regression
    // guard): the state-local validator tolerates absent bindings at
    // superseded (§1.5), so the edge is the only thing standing between an
    // in-flight item and a superseded row that silently dropped its
    // execution/outcome provenance.
    for erase in [0usize, 1] {
        let mut sup = equipped_item(State::InFlight, State::Superseded, 12);
        if erase == 0 {
            sup.execution_binding = None;
        } else {
            sup.outcome_binding = None;
        }
        // The state-local validator tolerates the same absence...
        assert!(
            validate_required_by_state(&sup, State::Superseded).is_ok(),
            "superseded must stay source-dependent at the state-local validator"
        );
        // ...and #12 is what rejects it.
        assert!(matches!(
            validate_transition(&sup, State::InFlight, State::Superseded, DriverRole::NickShape),
            Err(BacklogItemError::GuardUnsatisfied { edge: 12, .. })
        ));
    }

    // #11 carries the same both-bindings requirement into parked.
    for erase in [0usize, 1] {
        let mut parked = equipped_item(State::InFlight, State::Parked, 11);
        if erase == 0 {
            parked.execution_binding = None;
        } else {
            parked.outcome_binding = None;
        }
        assert!(matches!(
            validate_transition(&parked, State::InFlight, State::Parked, DriverRole::TrackDriver),
            Err(BacklogItemError::GuardUnsatisfied { edge: 11, .. })
        ));
    }
}

#[test]
fn an_undeclared_success_measure_is_not_a_declared_outcome_binding() {
    // The printed matrix marks `outcome_binding.success_measure_id (declared)`
    // R at in_flight and done, and §1.4 makes the DECLARED measure — not the
    // binding object — the thing #5 stamps and #11/#12 must carry. A binding
    // present with a null measure therefore satisfies nothing.
    for state in [State::InFlight, State::Done] {
        let mut item = equipped_item(State::Ready, State::InFlight, 5);
        item.state = state;
        if state == State::Done {
            normalize_on_state_entry(&mut item, State::Done);
        }
        assert!(
            validate_required_by_state(&item, state).is_ok(),
            "the declared-measure fixture must otherwise be complete at {state:?}"
        );
        item.outcome_binding.as_mut().unwrap().success_measure_id = None;
        assert!(
            matches!(
                validate_required_by_state(&item, state),
                Err(BacklogItemError::MissingRequiredField { ref field, .. })
                    if field == "outcome_binding.success_measure_id"
            ),
            "state-local validation at {state:?} must reject an undeclared success measure"
        );
    }

    // #5 cannot be authorized by an undeclared measure...
    let mut pickup = equipped_item(State::Ready, State::InFlight, 5);
    pickup.outcome_binding.as_mut().unwrap().success_measure_id = None;
    assert!(matches!(
        validate_transition(&pickup, State::Ready, State::InFlight, DriverRole::TrackDriver),
        Err(BacklogItemError::GuardUnsatisfied { edge: 5, .. })
    ));

    // ...and #11/#12 cannot "carry" one that was never declared.
    for (to, edge, role) in [
        (State::Parked, 11u8, DriverRole::TrackDriver),
        (State::Superseded, 12u8, DriverRole::NickShape),
    ] {
        let mut item = equipped_item(State::InFlight, to, edge);
        item.outcome_binding.as_mut().unwrap().success_measure_id = None;
        assert!(
            matches!(
                validate_transition(&item, State::InFlight, to, role),
                Err(BacklogItemError::GuardUnsatisfied { edge: e, .. }) if e == edge
            ),
            "#{edge} must reject an outcome binding that declares no success measure"
        );
    }
}

// --- done-rule (R22) --------------------------------------------------------

#[test]
fn done_rule_three_cases() {
    let mut ob = outcome_reading();
    ob.reading_status = ReadingStatus::Reading;
    assert!(done_rule_satisfied(&ob));
    ob.reading_status = ReadingStatus::UnmeasurableSigned;
    ob.nick_signoff = false;
    assert!(!done_rule_satisfied(&ob), "unmeasurable without signoff is not done");
    ob.nick_signoff = true;
    assert!(done_rule_satisfied(&ob));
    ob.reading_status = ReadingStatus::Registered;
    ob.nick_signoff = true;
    assert!(!done_rule_satisfied(&ob), "registered alone is not done");
}

// --- normalization (§1.8) ---------------------------------------------------

#[test]
fn normalize_constructs_parked_exit_from_staged_wake() {
    let mut item = equipped_item(State::Ready, State::Parked, 7);
    normalize_on_state_entry(&mut item, State::Parked);
    let exit = item.exit.as_ref().unwrap();
    assert_eq!(exit.kind, ExitKind::Parked);
    assert!(exit.wake_condition.is_some());
    assert!(exit.superseded_by.is_none() && exit.aged_out_reason.is_none());
    // rank + bindings preserved
    assert!(item.rank.is_some());
    assert!(item.execution_binding.is_some());
}

#[test]
fn normalize_resets_age_on_entry_to_ready_and_preserves_rank() {
    let mut item = equipped_item(State::Parked, State::Ready, 14);
    item.rank.as_mut().unwrap().inputs.age = 7;
    item.rank.as_mut().unwrap().position = 3;
    normalize_on_state_entry(&mut item, State::Ready);
    assert_eq!(item.state, State::Ready);
    assert_eq!(item.rank.as_ref().unwrap().inputs.age, 0, "advancement resets age");
    assert_eq!(item.rank.as_ref().unwrap().position, 3, "position not touched by normalize");
    assert!(item.exit.is_none(), "exit cleared entering ready");
}

#[test]
fn normalize_constructs_done_exit() {
    let mut item = equipped_item(State::InFlight, State::Done, 10);
    normalize_on_state_entry(&mut item, State::Done);
    let exit = item.exit.as_ref().unwrap();
    assert_eq!(exit.kind, ExitKind::Done);
    assert!(exit.wake_condition.is_none() && exit.superseded_by.is_none() && exit.aged_out_reason.is_none());
}

// --- policy parsers (R15/R25) -----------------------------------------------

#[test]
fn default_policy_matches_printed_order_and_budget() {
    let p = BacklogPolicy::default();
    assert_eq!(p.age_budget.get(), 3);
    assert_eq!(
        p.comparator,
        vec![
            ComparatorToken::ValueGapDesc,
            ComparatorToken::NickWeightDesc,
            ComparatorToken::AgeDesc,
            ComparatorToken::DependencyReadyFirst,
            ComparatorToken::EffortAsc,
        ]
    );
}

#[test]
fn comparator_parser_accepts_valid_and_trims() {
    let raw = " value_gap_desc, nick_weight_desc ,age_desc,dependency_ready_first, effort_asc ";
    let parsed = parse_comparator(raw).unwrap();
    assert_eq!(parsed, BacklogPolicy::default().comparator);
}

#[test]
fn comparator_parser_rejects_empty_unknown_duplicate_and_missing() {
    assert!(parse_comparator("").is_err(), "empty");
    assert!(parse_comparator("   ").is_err(), "whitespace-only");
    assert!(parse_comparator("value_gap_desc,bogus,age_desc,dependency_ready_first,effort_asc").is_err(), "unknown");
    assert!(parse_comparator("value_gap_desc,value_gap_desc,age_desc,dependency_ready_first,effort_asc").is_err(), "duplicate");
    assert!(parse_comparator("value_gap_desc,nick_weight_desc,age_desc").is_err(), "missing tokens");
    assert!(parse_comparator("Value_Gap_Desc,nick_weight_desc,age_desc,dependency_ready_first,effort_asc").is_err(), "case-sensitive");
}

#[test]
fn age_budget_parser_is_strict() {
    assert_eq!(parse_age_budget("5").unwrap().get(), 5);
    assert_eq!(parse_age_budget("  7 ").unwrap().get(), 7);
    assert!(parse_age_budget("0").is_err(), "zero");
    assert!(parse_age_budget("-1").is_err(), "sign");
    assert!(parse_age_budget("+1").is_err(), "sign");
    assert!(parse_age_budget("1 0").is_err(), "inner whitespace");
    assert!(parse_age_budget("99999999999999").is_err(), "overflow");
    assert!(parse_age_budget("").is_err(), "empty");
}

// --- comparator materialization (R15/R16) -----------------------------------

fn ranked_item(id: &str, mag: f64, weight: f64, age: u32, dep: DependencyStatus, effort: EffortClass, organ: &str) -> BacklogItem {
    let mut item = candidate_item();
    item.backlog_item_id = id.to_string();
    item.business_node_id = organ.to_string();
    item.state = State::Ready;
    item.effort_class = Some(effort);
    item.playbook_binding = Some(playbook_bound());
    item.rank = Some(full_rank(effort, dep, age, mag, weight));
    item
}

#[test]
fn materialize_rank_is_deterministic_and_reproducible() {
    let organ = format!("bn_{}", "a".repeat(10));
    let items = vec![
        ranked_item(&bi("aaaa"), 1.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
        ranked_item(&bi("bbbb"), 3.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
        ranked_item(&bi("cccc"), 2.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
    ];
    let p = BacklogPolicy::default();
    let a = materialize_rank(&items, &p);
    let b = materialize_rank(&items, &p);
    assert_eq!(a, b, "same items + comparator => identical positions");
    // highest value_gap first under the default comparator.
    assert_eq!(a[0].0, bi("bbbb"));
    assert_eq!(a[1].0, bi("cccc"));
    assert_eq!(a[2].0, bi("aaaa"));
    assert_eq!(a.iter().map(|(_, p)| *p).collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn materialize_rank_directional_monotonicity() {
    let organ = format!("bn_{}", "a".repeat(10));
    let base = vec![
        ranked_item(&bi("aaaa"), 1.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
        ranked_item(&bi("bbbb"), 1.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
    ];
    let p = BacklogPolicy::default();
    let pos_before = position_of(&materialize_rank(&base, &p), &bi("aaaa"));
    // Raise aaaa's value_gap; others fixed. Its position must be non-worse (<=).
    let mut raised = base.clone();
    raised[0].rank.as_mut().unwrap().inputs.value_gap_magnitude.magnitude = 9.0;
    let pos_after = position_of(&materialize_rank(&raised, &p), &bi("aaaa"));
    assert!(pos_after <= pos_before, "raising value_gap must not worsen position");
}

fn position_of(v: &[(String, u32)], id: &str) -> u32 {
    v.iter().find(|(i, _)| i == id).map(|(_, p)| *p).unwrap()
}

#[test]
fn materialize_rank_is_per_organ_scoped() {
    // Only the organ's own items are passed; a foreign organ never perturbs positions.
    let organ = format!("bn_{}", "a".repeat(10));
    let items = vec![
        ranked_item(&bi("aaaa"), 1.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
        ranked_item(&bi("bbbb"), 2.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
    ];
    let positions = materialize_rank(&items, &BacklogPolicy::default());
    assert_eq!(positions.len(), 2);
    assert_eq!(positions[0].0, bi("bbbb"));
}

#[test]
fn materialize_rank_excludes_non_candidate_ready_and_unranked() {
    let organ = format!("bn_{}", "a".repeat(10));
    let mut parked = ranked_item(&bi("aaaa"), 5.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ);
    parked.state = State::Parked;
    let mut unranked = ranked_item(&bi("bbbb"), 5.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ);
    unranked.rank = None;
    let ranked = ranked_item(&bi("cccc"), 1.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ);
    let items = vec![parked, unranked, ranked];
    let positions = materialize_rank(&items, &BacklogPolicy::default());
    assert_eq!(positions.len(), 1, "only ranked candidate/ready items are positioned");
    assert_eq!(positions[0].0, bi("cccc"));
}

#[test]
fn comparator_is_total_via_id_tie_break() {
    let organ = format!("bn_{}", "a".repeat(10));
    // identical inputs => the opaque id byte order decides, deterministically.
    let items = vec![
        ranked_item(&bi("bbbb"), 1.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
        ranked_item(&bi("aaaa"), 1.0, 1.0, 0, DependencyStatus::Ready, EffortClass::M, &organ),
    ];
    let positions = materialize_rank(&items, &BacklogPolicy::default());
    assert_eq!(positions[0].0, bi("aaaa"), "id byte order is the final total tie-break");
    assert_eq!(positions[1].0, bi("bbbb"));
}
