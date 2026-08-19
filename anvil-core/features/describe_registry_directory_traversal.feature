Feature: describe refuses to scan an out-of-hearth registry directory
  FileSystemDescribeAdapter::with_registry unions the core artifact
  directories with every directory a registered machine declares. A machine's
  `directory:` is authored text — in the playbook_generation builder flow it
  originates as raw LLM-generated `machine.yaml` — so a manipulated or buggy
  declaration could carry `..`, an absolute path, or an embedded separator.
  Joined in read_instance (`hearth/<directory>/<id>/status.yaml`), any such
  value escapes the hearth and turns describe into an arbitrary-file-read
  primitive. The adapter's trust boundary must refuse to scan any directory
  that is not a single path component, so these hostile declarations resolve to
  UnknownIdentifier instead of leaking a file outside the hearth. Drives the
  REAL FileSystemDescribeAdapter::with_registry.

  Scenario: an absolute-path machine directory cannot read a file outside the hearth
    Given a describe fs sandbox with a benign hearth entry "keep_me" and an out-of-hearth secret instance "escaped" in state "exfiltrated"
    When describe fs read_instance via a registry declaring the secret dir by absolute path for kind "malicious" is called for "escaped"
    Then the describe fs read is an UnknownIdentifier error

  Scenario: a parent-escape (..) machine directory cannot read a file outside the hearth
    Given a describe fs sandbox with a benign hearth entry "keep_me" and an out-of-hearth secret instance "escaped" in state "exfiltrated"
    When describe fs read_instance via a registry declaring directory "../secret" for kind "malicious" is called for "escaped"
    Then the describe fs read is an UnknownIdentifier error

  Scenario: a benign in-hearth instance still resolves through the same registry path
    Given a describe fs sandbox with a benign hearth entry "keep_me" and an out-of-hearth secret instance "escaped" in state "exfiltrated"
    When describe fs read_instance via a registry declaring directory "proposals" for kind "proposal" is called for "keep_me"
    Then the describe fs instance state is "draft"
    And the describe fs instance kind is "proposal"
