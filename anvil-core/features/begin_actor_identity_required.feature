Feature: Begin handler validates actor identity
  BeginCommandHandler validates the full actor identity before any
  filesystem side effects. Empty actor_name returns ActorNameRequired;
  empty actor_type / actor_model / actor_provider return
  ActorParamsRequired naming the missing field.

  Scenario: Empty actor_name returns ActorNameRequired
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    When begin is called via query adapter with empty actor_name and parent "20260411T2021_anvil_workflow_engine"
    Then the begin outcome is an ActorNameRequired error
    And the handler emitted no events

  Scenario: Empty actor_type returns ActorParamsRequired naming actor_type
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    When begin is called via query adapter with empty "actor_type" and parent "20260411T2021_anvil_workflow_engine"
    Then the begin outcome is an ActorParamsRequired error for "actor_type"
    And the handler emitted no events

  Scenario: Empty actor_model returns ActorParamsRequired naming actor_model
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    When begin is called via query adapter with empty "actor_model" and parent "20260411T2021_anvil_workflow_engine"
    Then the begin outcome is an ActorParamsRequired error for "actor_model"
    And the handler emitted no events

  Scenario: Empty actor_provider returns ActorParamsRequired naming actor_provider
    Given an in-memory query adapter with parent "20260411T2021_anvil_workflow_engine" in state "active"
    When begin is called via query adapter with empty "actor_provider" and parent "20260411T2021_anvil_workflow_engine"
    Then the begin outcome is an ActorParamsRequired error for "actor_provider"
    And the handler emitted no events
