Feature: A run's own record is readable

  A surface that shows someone their run has to read the run's real record. It
  must not re-derive one of its own: two copies of one fold is how two surfaces
  come to disagree about what happened.

  The record of a run is the steps it has taken — each one with the time it
  happened, the actor who took it, the part they were playing and, where one was
  required, the person who approved it. Work that was started from inside a run
  is part of that run's shape, so it comes back nested under it rather than
  flattened away.

  What a step cost is recorded nowhere in this engine. The record therefore
  carries no cost figure at all, rather than a zero that would read as a step
  that cost nothing.

  Scenario: A run's steps come back in the order they happened
    Given a hearth holding a run that has taken several steps
    When the run's record is asked for
    Then the run comes back with its steps in the order they happened
    And every step says when it happened and who took it
    And a step that needed approval names the person who approved it
    And no step carries a cost figure

  Scenario: Work started from inside a run comes back nested under it
    Given a hearth holding a run that has taken several steps
    And two further runs were started from inside that run
    When the run's record is asked for
    Then the runs started from inside it come back nested under it
    And each nested run carries its own steps
    And the run that was asked for is not itself nested under anything

  Scenario: Asking for a run that is not there is answered, not refused
    Given a hearth holding a run that has taken several steps
    When a record is asked for under a name no run has
    Then the answer says plainly that there is no such run
    And the answer carries no runs at all
