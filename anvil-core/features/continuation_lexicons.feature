Feature: Continuation lexicons — normalisation and the four predicates
  The continuation decision procedure (continuation_recognition, spec r12) is
  ordered and first-match-wins, and every one of its clauses reduces to a pure
  predicate over a normalised token sequence. Those predicates live here, in
  anvil-core, with no I/O: the engine composes them, the hook never evaluates
  them.

  Normalisation is one operation used EVERYWHERE this spec says "tokenise":
  NFKC, lowercase, typographic apostrophes folded to ASCII, split on whitespace
  and on `/` `\` and dashes, then Unicode-punctuation stripped from each token's
  EDGES only (so `don't` survives as one token). List prefixes are the sole
  exception — they read the RAW line, because the shared tokeniser destroys the
  very punctuation that marks them.

  Every example below was produced by a reviewer trying to break an earlier
  revision. Twelve rounds of constructed counter-examples are the regression
  suite; keeping them as data is the only thing that stops the boundary-finding
  having to be repeated.

  Scenario Outline: normalisation folds, splits and strips
    When the continuation tokeniser runs on "<message>"
    Then the continuation tokens are "<tokens>"

    Examples:
      | message              | tokens              |
      | don’t                | don't               |
      | don't                | don't               |
      | no/problem           | no,problem          |
      | actually-start       | actually,start      |
      | ¿no?                 | no                  |
      | corridor             | corridor            |
      | Great, do it         | great,do,it         |
      |   go                 | go                  |

  # The rejection set is deliberately OVER-BROAD. A missed continuation costs one
  # turn of guidance; a mis-routed rejection actions work the human refused.
  Scenario Outline: rejection markers are recognised
    When the continuation tokeniser runs on "<message>"
    Then the message is a rejection

    Examples:
      | message                      |
      | No, abandon it               |
      | don't stop, continue         |
      | no problem, do it            |
      | not the first one            |
      | actually, do it              |

  Scenario Outline: ordinary continuations are not rejections
    When the continuation tokeniser runs on "<message>"
    Then the message is not a rejection

    Examples:
      | message                  |
      | Great do it              |
      | continue please          |
      | keep going               |
      | ship it                  |

  # `first` and `maybe` were deferral markers in revision 6 and are NOT here:
  # they are ordinary sequencing language, and treating them as vetoes silenced
  # "First, run the tests."
  Scenario Outline: deferral markers are recognised
    When the continuation tokeniser runs on "<message>"
    Then the message is a deferral

    Examples:
      | message           |
      | Later—keep going  |
      | Hold on           |
      | Wait              |
      | Pause please      |
      | I need a break    |
      | I'm confused      |

  Scenario Outline: sequencing language is not deferral
    When the continuation tokeniser runs on "<message>"
    Then the message is not a deferral

    Examples:
      | message               |
      | First, run the tests  |
      | Fix this first        |
      | Maybe fix that        |

  # PURE CONSENT is what a prior proposal is allowed to preempt: a message
  # carrying NO independent action content. It bounds token COUNTS as well as
  # membership, because "Yes, do this, do that" is entirely in-vocabulary and
  # still directs two things.
  Scenario Outline: pure consent lets a proposal decide
    When the continuation tokeniser runs on "<message>"
    Then the message is pure consent

    Examples:
      | message            |
      | yes                |
      | Great do it        |
      | ok do that         |
      | Yes, do it now     |
      | Yes, go ahead      |
      | Please do it now   |

  Scenario Outline: a message with its own action content is not pure consent
    When the continuation tokeniser runs on "<message>"
    Then the message is not pure consent

    Examples:
      | message                 |
      | Okay, ship the fix      |
      | Sure, send it           |
      | Go fix it               |
      | Good going              |
      | Do this and then that   |
      | Yes, do this, do that   |

  # The question test is a two-token SHAPE, not a first-token lookup. WH words
  # lead a question alone; auxiliaries only before a pronoun — which is what
  # keeps "Will do" an answer and makes "Do you continue" a question. `it` is
  # excluded from that pronoun set so "Do it" stays imperative.
  #
  # The last two rows exist to isolate the `?` guard specifically: neither is
  # interrogative-SHAPED, so only "contains a question mark ANYWHERE" can catch
  # them. Mutation-checked — reverting that guard to a positional `ends_with`
  # leaves every other row passing, which is how a spec requirement (round-6
  # finding 3) would have silently regressed.
  Scenario Outline: interrogative shapes are questions
    When the continuation tokeniser runs on "<message>"
    Then the message is a question

    Examples:
      | message               |
      | Are you sure?         |
      | What did you mean?    |
      | Will you continue     |
      | May I proceed         |
      | Do you continue       |
      | Am I doing this right |
      | Okay?                 |
      | ship it? maybe        |
      | sounds good? 🙂       |

  Scenario Outline: imperatives and answers are not questions
    When the continuation tokeniser runs on "<message>"
    Then the message is not a question

    Examples:
      | message      |
      | Will do      |
      | Can do       |
      | Do it        |
      | Great do it  |
      | go           |

  # Step 7 widens on POSITIVE evidence — a directive, or a message that is
  # wholly agreement — never on the mere absence of objection. "Good point."
  # evaluates something; it does not ask for anything.
  Scenario Outline: affirmative messages widen
    When the continuation tokeniser runs on "<message>"
    Then the message is affirmative

    Examples:
      | message                        |
      | Great do it                    |
      | continue please                |
      | let's get them fixed please    |
      | Continue from where you left off |
      | sounds good                    |
      | yep                            |

  Scenario Outline: evaluation and interruption are not affirmative
    When the continuation tokeniser runs on "<message>"
    Then the message is not affirmative

    Examples:
      | message         |
      | Good point      |
      | I'm confused    |
      | Are you sure?   |
      | did you run the tests |

  # ---- prior_proposal: SYNTACTIC extraction (plan phase 3) -----------------
  # Shape only. Whether <X> resolves to a granted kind is the ENGINE's job — the
  # hook cannot know the granted set, and a second matcher here is the one
  # duplication the plan forbids.
  Scenario Outline: a single unambiguous proposal is extracted
    When the prior proposal is extracted from "<turn>"
    Then the extracted proposal text is "<text>"

    Examples:
      | turn                                      | text                       |
      | Shall I record this as a decision?        | record this as a decision  |
      | Want me to start a track?                 | start a track              |
      | I'd take telemetry                        | telemetry                  |
      | Next: record this as a decision           | record this as a decision  |
      | Some prose.\nShall I start a track?       | start a track              |

  # Every row below is a case a reviewer built to break an earlier revision.
  Scenario Outline: ambiguous, quoted, structural or negated lines are declined
    When the prior proposal is extracted from "<turn>"
    Then no prior proposal is extracted

    Examples:
      | turn                                             |
      | I can fix routing, or add telemetry              |
      | Next: fix routing or add telemetry               |
      | Next: do not amend it                            |
      | # Next: record this as a decision                |
      | Shall I proceed? was the prompt                  |
      | 4. Fix routing\nNext: record this as a decision  |
      | 1) Fix routing\nNext: record this as a decision  |
      | A. Fix routing\nNext: record this as a decision  |
      | • Fix routing\nNext: record this as a decision   |
      | Just a status update.                            |

  # A table cell cannot carry an escaped pipe through brine's parser, so this
  # guard gets its own scenario rather than a row that silently degrades.
  Scenario: a table row containing a proposal form is not a proposal
    When the prior proposal is extracted from "| Next: record this as a decision |"
    Then no prior proposal is extracted

  # ---- the ordered decision procedure (plan phase 4) -----------------------
  # The ORDER is the design. It was wrong twice across the review series, in
  # ways that defeated the track's own premise, and the four rows marked
  # REGRESSION are what stop either error returning.
  Scenario Outline: the procedure resolves in order, first match wins
    Given an open "<open_kind>" run, new_intent "<new_intent>", context "<context>", proposal "<proposal>"
    When the continuation procedure runs on "<message>"
    Then the continuation outcome is "<outcome>"

    Examples:
      | message                     | open_kind | new_intent | context | proposal | outcome           |
      | a b c d e f g               | track     | no         | yes     |          | route_normally    |
      |                             | track     | no         | yes     |          | route_normally    |
      | Great do it                 |           | no         | yes     |          | route_normally    |
      | start a track to fix X      | track     | yes        | yes     |          | route_normally    |
      | No, abandon it              | track     | no         | yes     | decision | rejected          |
      | yes                         | track     | no         | yes     | decision | redirect:decision |
      | yes                         | track     | no         | yes     | track    | resume_contextual |
      | go                          | track     | no         | yes     |          | resume            |
      | Great do it                 | track     | no         | yes     |          | resume_widened    |
      | Great do it                 | track     | no         | no      |          | route_normally    |
      | Good point                  | track     | no         | yes     |          | route_normally    |

  # REGRESSION (round 5): a proposal must never preempt an explicit resumption.
  # Revision 5 redirected this to the decision, against a message that defers the
  # proposal and asks to continue.
  Scenario: an explicit resumption beats a cross-kind proposal
    Given an open "track" run, new_intent "no", context "yes", proposal "decision"
    When the continuation procedure runs on "Later—keep going"
    Then the continuation outcome is "resume_widened"

  # REGRESSION (round 6): consent-plus-action names its own object, so the
  # PROPOSAL must not decide. It still advances the OPEN run — the human asked
  # for work, there is a run open, and nothing else matched. What this pins is
  # that it never becomes redirect:decision.
  Scenario: a message with its own action content is not preempted by a proposal
    Given an open "track" run, new_intent "no", context "yes", proposal "decision"
    When the continuation procedure runs on "Okay, ship the fix"
    Then the continuation outcome is "resume_widened"

  # REGRESSION (round 8): the guards bind to BOTH arms of step 7. Written without
  # the parentheses, a directive token resumed despite a deferral AND a question.
  Scenario: a deferral plus a question never widens
    Given an open "track" run, new_intent "no", context "yes", proposal ""
    When the continuation procedure runs on "Pause, then continue?"
    Then the continuation outcome is "route_normally"

  # REGRESSION (round 11): consent with an adverb is still consent.
  Scenario: modified consent still lets the proposal decide
    Given an open "track" run, new_intent "no", context "yes", proposal "decision"
    When the continuation procedure runs on "Yes, do it now"
    Then the continuation outcome is "redirect:decision"
