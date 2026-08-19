# Anvil Philosophy

## Agents are transient; intent is not

An agent lives for one conversation. It is born, reads what context it can, does work, and dies. Everything it learned — every dead end it explored, every architectural constraint it discovered, every decision it made — dies with it unless the system captures it explicitly.

The human is the only persistent actor. They carry the vision of what the system should become. But they can't hold every detail, and they can't be present for every agent's every question. Anvil exists to make human intent survive agent death — to let a fleet of transient agents act coherently toward an outcome that none of them will live long enough to see completed.

This is the fundamental asymmetry anvil is designed around. Humans have continuity but limited bandwidth. Agents have bandwidth but no continuity. Anvil trades between them: the human articulates intent once, anvil preserves it, and each new agent inherits just enough context to act as if it remembers what it never experienced.

## Anvil is a communication system

Anvil is not a project management tool. It is not a knowledge base. It is a communication system optimized for a specific relationship: a human directing transient agents toward a particular outcome.

Every artifact anvil produces exists to answer a question that a newly-spawned agent would ask:

- **What are we trying to achieve?** Visions and proposals articulate the desired future state and the chosen approach. They are the human's intent, written precisely enough that an agent who has never seen the codebase can orient itself.

- **What exactly should I build?** Specs and plans translate strategic intent into concrete tasks. A spec tells the agent what the contract is. A plan tells it what order to work in and where the last agent left off.

- **What has already been tried?** Decisions record what was evaluated and rejected. Tensions record what remains unresolved. Without these, agents re-explore paths that prior agents already exhausted — burning human attention on reviews of work that was already judged unnecessary.

- **What should I not do?** Truth invariants and initiatives are constraints the agent must respect. They are the accumulated lessons of prior work, pushed into every agent's context because violating them is costlier than the context window space they consume.

- **Am I doing it right?** Reviews verify that execution matches intent. They are the feedback loop that catches drift before it compounds across agent lifetimes.

These are not five systems bolted together. They are five facets of a single problem: how does a persistent human communicate effectively with agents that forget everything?

## Intent degrades through a funnel

Human intent starts abstract and must become concrete enough for an agent to execute. Each stage of the lifecycle is a refinement step in that funnel:

A **vision** names the problem and the desired world. It says nothing about how to get there. This is the broadest expression of intent — legible to any agent regardless of what part of the system it works on.

A **proposal** commits to an approach. It narrows the vision to a specific strategy, recording what alternatives were considered and why this one was chosen. The narrowing is itself a decision — and the rejected alternatives are as valuable as the chosen one, because they prevent future agents from re-proposing what was already evaluated.

A **spec** defines the contract for a single track of work. It is precise enough that two different agents, reading the same spec on different days, would produce compatible implementations. Where the proposal says "we'll use the immutable context pipeline," the spec says "map steps take owned context and return new context; check steps borrow immutably."

A **plan** sequences the work into tasks an agent can complete in a single session. It tracks progress with commit SHAs, so the next agent knows exactly where to resume.

At each stage, the human reviews and approves before the funnel narrows further. This is not bureaucracy — it is the mechanism by which intent is verified before it becomes harder to change. A misunderstanding caught at the vision stage costs a conversation. A misunderstanding caught at implementation costs days of agent work and a human's attention to untangle it.

## Decisions are the invisible load-bearing structure

The most dangerous failure mode in a fleet of transient agents is not a bad implementation. It is a good implementation of something that was already evaluated and rejected.

An agent considering whether to decompose a skill will grep the codebase, see that the skill is 194 lines, and begin writing a spec. It has no way of knowing that a prior agent already conducted a thorough assessment, established a five-criterion evaluation framework, and concluded that decomposition wasn't warranted. That prior work is invisible — it lives in a completed track's spec text, legible only to someone who already knows it exists.

Decisions — both "we chose X" and "we chose not to do X" — are the institutional memory that agents lack by nature. A decision carries its rationale, its alternatives, and its validity conditions: the circumstances under which it should be revisited. Without decisions as a first-class artifact, the system captures what was built but not why, and every new agent must re-derive the reasoning from first principles or stumble into the same dead ends.

This is not a knowledge management problem in the abstract. It is a communication problem specific to transient agents: the human made a judgment, and that judgment must be discoverable by agents who weren't present for it.

## Push and pull serve different moments

Not all context is needed at all times. Anvil distinguishes between what every agent must know and what an agent needs only when entering specific territory.

**Push** — truth invariants and active initiatives — is injected into every agent's context at conversation start. These are constraints so fundamental that violating them would waste more time than reading them costs. An agent implementing a new runner must know about the immutable context pipeline. An agent writing a spec must know about the feature-file-as-contract rule. Push context is the cost of orientation, paid once per agent lifetime.

**Pull** — decisions, tensions, and prior deliberation — is searched for when an agent is about to commit effort in a domain where prior work may exist. An agent about to envision a new direction searches for existing sparks and tensions. An agent about to spec a design searches for decisions already made in the area. Pull context is the cost of due diligence, paid per phase.

The distinction matters because agent context windows are finite. Pushing everything wastes attention on irrelevant constraints. Pulling nothing risks re-exploring settled territory. Anvil makes the choice explicit: what's load-bearing gets pushed; what's situational gets pulled.

## The state machine is a memory prosthetic

`status.yaml` is not project management overhead. It is the mechanism by which an agent born thirty seconds ago can determine exactly where a multi-session effort stands.

The last agent transitioned to `implementing`, committed through phase 2 task 3 with SHA `a1b2c3d`, and marked a checkpoint. The next agent reads this and picks up at phase 2 task 4. Without the state machine, the human would need to re-explain progress at the start of every conversation — or worse, the agent would infer progress from the code and get it subtly wrong.

Every transition records who did it, what role they played, and who authorized it. This is not auditing for its own sake. It is context for the next agent: if the last reviewer flagged a concern, the implementing agent needs to know. If the human approved an exception, the reviewing agent needs to know. The transition log is the conversation history that agents lose at death, reconstructed from structured events.

The bracket model — transition opens, commit closes — pairs intent with evidence. The transition says "this phase started." The commit says "this phase's work is done and in the repository." If the bracket is open (transition without a closing commit), the next agent knows the prior session ended without completing its work. No ambiguity, no inference from partial state.

## Reviews close the loop between intent and execution

A transient agent can drift from intent without noticing. It reads the spec, forms an understanding, and implements based on that understanding. If its understanding diverged from the human's intent — or from constraints established by prior work — the divergence compounds silently across tasks and phases.

Reviews are not quality gates in the traditional sense. They are the mechanism by which the human (or a reviewer agent) verifies that the communication worked. Did the implementing agent understand the spec? Did the speccing agent understand the proposal? Did the proposal capture the vision? Each review is a check on the fidelity of the intent funnel.

This is why every finding requires an explicit disposition. A silent skip is a communication failure — the reviewer raised a concern and the author didn't acknowledge it. Whether the disposition is "will address" or "acknowledged, not addressing because X," the loop must close. Open loops between transient agents never close on their own, because the agents that could close them are already dead.

## The human is the only authority

Agents propose. Reviewers evaluate. Only humans decide.

This is not a limitation of the technology. It is a design principle rooted in the transience problem. An agent cannot carry the full context of a project's history, stakeholder constraints, and strategic direction. It sees what's in its context window. The human sees the project as a continuous thread — every conversation, every decision, every course correction accumulated over time.

State transitions that carry judgment — activating a proposal, completing a milestone, approving an exception, reordering priorities — are reserved for the human. Not because agents can't form opinions, but because agents can't carry the context that makes those opinions trustworthy across the full timeline of a project.

Anvil encodes this structurally. Agents transition to their own working states. Humans authorize transitions to the next phase. The separation is not enforced by access control — it is enforced by the lifecycle itself: an agent that transitions beyond its own phase produces an artifact that no other agent will trust, because the approval record is missing.

## The infrastructure must be invisible

Anvil's value is in the communication it enables, not in the mechanisms it uses. An agent searching for prior decisions should express intent ("find decisions about adapter dispatch"), not mechanism ("grep for 'adapter' in decisions.md"). The access contract must be stable even as the storage evolves.

Today anvil runs on flat files in git. This works because agents can read and write markdown. Tomorrow it may run on a local event store for richer queries. Eventually it may run on a streaming engine for reactive projections. The agents should not know or care. The question "what has been decided about adapter dispatch?" has the same answer regardless of whether it's resolved by grepping a markdown file or querying a database.

Git remains the distribution protocol at every stage. Each clone has the complete knowledge system. No central server, no availability concerns, no deployment. Anvil must be as portable as the repository it lives in — because an agent's lifetime is one conversation, and the overhead of connecting to external infrastructure is overhead stolen from the work.

The flat-file implementation is the proving ground. It validates the patterns — event sourcing, deterministic projections, structured transitions, pull-based discovery — in the simplest possible medium. When the patterns are proven, the infrastructure can evolve without changing what agents see. The communication system is the product. The storage is an implementation detail.
