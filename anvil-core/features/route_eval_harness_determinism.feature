Feature: The router eval harness is deterministic and re-runnable over a fixed trace set
  router_precision Phase 3 — the eval-harness-over-real-traces contract. The whole
  self-improving loop (score baseline -> propose variant -> score variant -> decide)
  is only trustworthy if the router is a PURE, deterministic function of its inputs:
  the same trace (message + ctx) against the same registry must always produce the
  same resolution. Otherwise a score delta between baseline and variant could be
  resolver nondeterminism, not a real routing change, and the decide gate would be
  measuring noise.

  This is the property the corpus scorer (anvil_router_judge.py, driving the engine
  Route RPC over anvil_router_corpus.jsonl) depends on: it never sets a seed, never
  averages repeated calls — it assumes one Route call per case is representative.
  These scenarios prove that assumption at the resolver seam, over the real fixture
  registry, no mocks.

  Background:
    Given a routable playbooks fixture registry with authored route triggers
    And a route resolver request context with org "Foundation" role "read" clearance "internal" space ""

  Scenario Outline: a fixed trace resolves identically on every re-run
    When the pure route resolver runs with input "<message>"
    Then re-running the resolver 8 times over the same input yields an identical resolution

    # One trace per outcome class so the determinism guarantee spans the whole
    # resolver: a clean single, a candidate set, and an abstention.
    Examples:
      | message                                                                   |
      | start a track to implement the new caching layer                          |
      | compare my topic and the evidence                                         |
      | run the test suite and tell me what fails                                 |
      | answer this from my notes: what do I know about transformer scaling?      |
      | show me that topic                                                        |
