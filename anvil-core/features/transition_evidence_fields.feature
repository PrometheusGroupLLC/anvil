Feature: transition record carries claim status and artifact assessment
  `TransitionMeasurementRecord` gains two fields. They answer DIFFERENT
  questions, and the reason there are two is that the consumer's predicate is a
  conjunction — unclaimed AND placeholder — which a claim-only field cannot
  express. Shipping just the claim would force the reader to reject every
  unclaimed transition (re-creating the 73 false positives this track exists to
  avoid) or none of them.

  Both are optional on read. Records written before these fields existed decode
  unchanged, and a record with nothing to say omits them entirely rather than
  writing an empty string — so an old row and a new silent row are byte-identical.

  Neither value is ever the string `absent`. That value is in the downstream
  scorer's FORBIDDEN_VALUES and would fail its frozen validation on every
  instrumented record — a rename that had to happen in the definition AND
  everywhere else that named the old thing.

  Scenario: a record carrying both fields round-trips through the sink
    Given a transition measurement record with claim status "unclaimed" and artifact assessment "placeholder"
    When the transition measurement sink is read back
    Then the transition record claim status is "unclaimed"
    And the transition record artifact assessment is "placeholder"

  Scenario Outline: every declared value survives the round trip
    Given a transition measurement record with claim status "<claim>" and artifact assessment "<artifact>"
    When the transition measurement sink is read back
    Then the transition record claim status is "<claim>"
    And the transition record artifact assessment is "<artifact>"

    Examples:
      | claim          | artifact       |
      | claimed        | substantive    |
      | self_described | placeholder    |
      | unclaimed      | missing        |
      | pending        | not_applicable |

  # The migration case: a row from before the fields exist must still decode.
  Scenario: a record written without the fields reads back with neither
    Given a transition measurement record with no evidence fields
    When the transition measurement sink is read back
    Then the transition record has no claim status
    And the transition record has no artifact assessment

  # ---- the five-state claim model (plan phase 3) ---------------------------
  # Ordered, first match wins. `self_described` exists because SelfDescription is
  # the #[default] evidence class — folding it into `claimed` would let the
  # DEFAULT VALUE certify the work.
  Scenario Outline: the claim model resolves in order
    When the claim is classified with begin "<begin>" artifact_of_record "<aor>" classes "<classes>"
    Then the claim status is "<status>"

    Examples:
      | begin | aor | classes                          | status         |
      | yes   | yes | artifact_of_consequence          | pending        |
      | no    | no  | artifact_of_consequence          | not_applicable |
      | no    | yes | artifact_of_consequence          | claimed        |
      | no    | yes | verifiable_citation              | claimed        |
      | no    | yes | self_description,artifact_of_consequence | claimed |
      | no    | yes | self_description                 | self_described |
      | no    | yes | self_description,self_description | self_described |
      | no    | yes |                                  | unclaimed      |

  # A begin records pending EVEN WITH a strong claim: the actor is entering the
  # state, not leaving it, and cannot have authored the artifact of a state they
  # are only now arriving in. An earlier design assessed the state being ENTERED
  # and would have accused every legitimate new run at creation.
  Scenario: a begin is pending regardless of what it claimed
    When the claim is classified with begin "yes" artifact_of_record "yes" classes "verifiable_citation"
    Then the claim status is "pending"

  # ---- the two-condition warning gate --------------------------------------
  # BOTH conditions, never one. That is what separates the 8 genuine
  # abandonments from the 73 unclaimed-but-real completions measured on the live
  # fleet — a ~9:1 false-positive rate if either half were dropped.
  Scenario Outline: the warning needs unclaimed AND a placeholder or missing artifact
    When the warning gate is evaluated for claim "<claim>" artifact "<artifact>"
    Then the warning fires is "<warns>"

    Examples:
      | claim          | artifact       | warns |
      | unclaimed      | placeholder    | yes   |
      | unclaimed      | missing        | yes   |
      | unclaimed      | substantive    | no    |
      | self_described | placeholder    | no    |
      | claimed        | placeholder    | no    |
      | pending        | placeholder    | no    |
      | not_applicable | not_applicable | no    |

  # THE REGRESSION TEST FOR THE 73. This is the case that indicts correct work if
  # got wrong: someone completed a step, wrote a real artifact, and simply did
  # not pass an evidence argument. It must stay silent.
  Scenario: an unclaimed completion over real content never warns
    When the warning gate is evaluated for claim "unclaimed" artifact "substantive"
    Then the warning fires is "no"

  # ---- placeholder detection without the display name ----------------------
  # The display name is not available at the transition emit site, so the
  # predicate tests membership in the IMAGE of the bootstrap function rather
  # than equality with one rendered string. Derived from the writer, so a change
  # to the scaffold format changes this with it.
  Scenario Outline: an untouched scaffold is recognised whatever it was named
    When the file content "<content>" is tested as a bootstrap placeholder for "track" "spec"
    Then it is a placeholder is "<verdict>"

    Examples:
      | content                        | verdict |
      | # Alpha Track\n                | yes     |
      | # Something Else Entirely\n    | yes     |
      | # A\n                          | yes     |
      | # Alpha Track\n\nReal body.\n  | no      |
      | Real body with no heading.\n   | no      |
      |                                | no      |
      | # Alpha Track                  | no      |

  # THE CASE THAT MUST NOT WARN: a genuinely short spec someone wrote. It has the
  # heading shape but real content after it, so it reads as substantive.
  Scenario: a short but real spec is not a placeholder
    When the file content "# Fix\n\nDo the thing.\n" is tested as a bootstrap placeholder for "track" "spec"
    Then it is a placeholder is "no"
