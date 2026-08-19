---
name: workflow_generation:analyze
description: Analyze gathered playbook evidence into phases, roles, review gates, and outcome-linked exemplar bands for workflow_generation.
---

# Analyze: mine the process and outcome bands

Use this skill in the workflow_generation `analyzing` state after gathering has
captured the real process and outcome-ledger identification.

## Inputs

Read the workflow_generation artifact's gathered evidence and proposal notes.
Use the declared `corpus`, `ledger`, and `ledger_classification` from gathering.

## Output

Write the analysis surface for modeling: phase/role/gate structure, candidate
state names, artifacts per state, revision loops, and outcome-linked exemplar
mining results.

## Procedure

1. Build the process skeleton: ordered work states, review gates, revision paths,
   terminal state, roles, and durable artifacts.
2. Join each candidate historical instance from the corpus to its outcome ledger
   record or proxy.
3. Sort joined instances into the four bands:
   - `good`: process pattern and outcome are both good.
   - `bad`: process pattern and outcome are both poor.
   - `trap`: process pattern looks good but the downstream outcome is poor.
   - `hidden_virtue`: process pattern looks rough but the downstream outcome is good.
4. Prioritize traps first. These are the most important anchors because they
   prevent the generated playbook from rewarding polished but ineffective work.
5. Select 1-2 distilled exemplar candidates per important rubric dimension when
   the ledger allows it. Use only redacted/distilled patterns, never raw sensitive
   artifacts.
6. If `ledger_classification` is `none_yet`, carry the complete
   `none_yet_justification` forward and state which rubric dimensions lack
   anchors.

The modeling step must be able to populate `success_rubric`, `anchors`, and
exemplar markdown files directly from this analysis without re-mining the corpus.
