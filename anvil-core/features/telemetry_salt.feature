Feature: Salted actor hashing for the step-measurement sink
  The step-measurement sink records a salted, non-reversible, truncated SHA-256
  of the actor name so distinct-actor counts can be computed without storing a
  raw or unsalted identity. The salt is per-deployment, so the same actor hashes
  identically everywhere — which is what makes cross-hearth distinct counts
  correct. When no salt is configured the hash is absent (fail-safe): no actor
  counting, never a leak.

  The same salt is also fingerprinted into a KEY EPOCH — a domain-separated,
  12-hex-char digest of the salt itself — so two reports can declare whether they
  were computed in the same keyspace without either of them carrying the salt.
  A keyspace split and genuine non-adoption produce identical-looking numbers,
  and the epoch is what tells them apart.

  Scenario: the same actor and salt always produce the same hash
    Given the telemetry salt "deployment-salt-123"
    When the actor "Falcon-004217" is hashed
    And the actor "Falcon-004217" is hashed again
    Then both actor hashes are equal
    And the actor hash is a non-empty 16-character hex string

  Scenario: a different salt produces a different hash for the same actor
    Given the telemetry salt "salt-one"
    When the actor "Falcon-004217" is hashed
    And the telemetry salt is changed to "salt-two"
    And the actor "Falcon-004217" is hashed again
    Then the two actor hashes differ

  Scenario: no salt configured yields no actor hash
    Given no telemetry salt is configured
    When the actor "Falcon-004217" is hashed
    Then the actor hash is absent

  Scenario: an empty actor yields no actor hash even with a salt
    Given the telemetry salt "deployment-salt-123"
    When the actor "" is hashed
    Then the actor hash is absent

  # The key epoch fingerprints the DEPLOYMENT salt itself, so two reports can be
  # compared only when they were computed in the same keyspace. It is the
  # comparability key between reports, which is why it must be deterministic.
  Scenario: the same salt always fingerprints to the same key epoch
    Given the telemetry salt "deployment-salt-123"
    When the key epoch is fingerprinted
    And the key epoch is fingerprinted again
    Then both key epochs are equal

  # Consumers compare epochs as opaque strings, so the encoding is contractual:
  # a different truncation, uppercase, or a non-hex encoding all read as a
  # different keyspace.
  Scenario: the key epoch is exactly 12 lowercase hex characters
    Given the telemetry salt "deployment-salt-123"
    When the key epoch is fingerprinted
    Then the key epoch is a non-empty 12-character lowercase hex string

  # An epoch that is constant across salts reports every keyspace as the same
  # one, which is the failure that makes a keyspace split look like non-adoption.
  Scenario: two different salts fingerprint to two different key epochs
    Given the telemetry salt "salt-one"
    When the key epoch is fingerprinted
    And the key epoch is fingerprinted for salt "salt-two"
    Then the two key epochs differ

  # Domain separation. An epoch and a conversation hash must never be able to
  # collide in one value space: the epoch is neither the actor hash of the salt
  # under itself (the truncated-actor_hash implementation) nor the same digest
  # with the domain dropped from its input.
  Scenario: the key epoch of a salt is not its actor hash and is not an undomained digest
    Given the telemetry salt "deployment-salt-123"
    When the key epoch is fingerprinted
    Then the key epoch is not the actor hash of the salt, nor a prefix or suffix of it
    And the key epoch is not the undomained digest of the salt

  # A report field that is silently absent reads as "not applicable"; the absent
  # keyspace has to be nameable, so both absence shapes yield one sentinel.
  Scenario: no salt and an empty salt both yield the unknown key-epoch sentinel
    Given no telemetry salt is configured
    When the key epoch is fingerprinted
    And the key epoch is fingerprinted for salt ""
    Then both key epochs are equal
    And the key epoch is the unknown key-epoch sentinel

  # The non-reversibility floor, asserted at the cheapest seam there is: a
  # passthrough or an encode-instead-of-hash leaves the salt visible in the epoch.
  Scenario: the key epoch never contains the salt as a substring
    Given the telemetry salt "beef"
    When the key epoch is fingerprinted
    Then the key epoch is a non-empty 12-character lowercase hex string
    And the key epoch does not contain the salt
