Feature: FileSystemQueryAdapter resolves a domain-machine artifact kind
  The FileSystem query adapter must resolve the kind and state of an
  artifact stored under a domain-machine directory (e.g. knowledge/<id>)
  even though the kind is NOT in the legacy 6-kind KIND_DIRS list. The
  authoritative kind lives in the artifact's own status.yaml; directory
  discovery scans the hearth for the matching <id> dir containing a
  status.yaml. (S5 / AC5.)

  Scenario: read_artifact_kind resolves knowledge_lifecycle by bare id
    Given a domain-kind fs hearth with:
      | path                                          | content                                              |
      | knowledge/20260601T0000_topic/status.yaml     | version: 1\nkind: knowledge_lifecycle\nstate: ingesting\n |
    When fs_query_adapter.read_artifact_kind is called for "20260601T0000_topic"
    Then the fs domain query adapter returns kind "knowledge_lifecycle"

  Scenario: read_artifact_kind resolves knowledge_lifecycle by literal path
    Given a domain-kind fs hearth with:
      | path                                          | content                                              |
      | knowledge/20260601T0000_topic/status.yaml     | version: 1\nkind: knowledge_lifecycle\nstate: ingesting\n |
    When fs_query_adapter.read_artifact_kind is called for "knowledge/20260601T0000_topic"
    Then the fs domain query adapter returns kind "knowledge_lifecycle"

  Scenario: read_artifact_state resolves the domain artifact's state
    Given a domain-kind fs hearth with:
      | path                                          | content                                              |
      | knowledge/20260601T0000_topic/status.yaml     | version: 1\nkind: knowledge_lifecycle\nstate: ingesting\n |
    When fs_query_adapter.read_artifact_state is called for "20260601T0000_topic"
    Then the fs domain query adapter returns state "ingesting"

  Scenario: a legacy track artifact still resolves kind track (behavior-preservation)
    Given a domain-kind fs hearth with:
      | path                                          | content                                              |
      | tracks/20260601T0000_legacy/status.yaml       | version: 1\nkind: track\nstate: spec\n               |
    When fs_query_adapter.read_artifact_kind is called for "20260601T0000_legacy"
    Then the fs domain query adapter returns kind "track"
