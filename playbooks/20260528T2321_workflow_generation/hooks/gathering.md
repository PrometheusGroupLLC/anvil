# Gathering: elicit the real activity and its outcome ledger

You are authoring a new playbook by running this builder. The `gathering` step
extracts the real process from the domain owner and identifies how outcomes are
later known. A playbook that cannot point at outcomes cannot be made measurable.

## What to produce

Captured domain notes written to this playbook's evidence: the activity's real
steps, roles, consumed and produced artifacts, review/revision points, and the
outcome-ledger identification needed by analysis and modeling.

Include an outcome-ledger section with:

- `corpus`: the body of past instances or comparable work searched.
- `ledger`: the system that records outcomes, or the closest available proxy.
- `ledger_classification`: `has_examples`, `partial`, or `none_yet`.
- `none_yet_justification` when classification is `none_yet`: corpus searched,
  ledger searched, why no exemplar exists, whether production routing is allowed,
  and the follow-up condition that would make exemplars available.

## How to elicit

Walk the owner through the actual process end to end. Anchor on three moves and
keep asking "and then what happens?" until the process terminates:

1. **Extract** — How does raw input enter the process? What triggers a new unit
   of work? What does the owner physically do first, and with what tool or
   artifact? Get a real recent example, not the abstract policy.

2. **Connect** — How does one step's output become the next step's input? Where
   does work wait, get reviewed, or loop back? Which decisions branch the flow,
   and what does the owner inspect to decide?

3. **Commit** — When is the unit of work done? What makes it acceptable vs.
   sent back? Who signs off, and against what standard? What terminal artifact
   or external record outlives the process?

## Outcome-ledger probes

- "Where would we look later to know this playbook actually helped?"
- "Show me examples with known good outcomes and known bad outcomes."
- "Which failures look superficially successful until a later ledger disproves them?"
- "Which messy or surprising examples produced good downstream outcomes?"
- "If no outcome ledger exists yet, what exactly did you search and what future
  signal would make routing safe?"

Capture the messy reality, including rework loops, informal reviews, implicit
acceptance criteria, and ledger gaps. Those become review gates, rubric
dimensions, anchors, and bounded escape hatches downstream.
