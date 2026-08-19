Feature: MCP Handshake
  The MCP shim implements the JSON-RPC 2.0 protocol for MCP initialization.

  Scenario: Initialize handshake returns server capabilities
    Given the MCP shim is started
    When an initialize request is sent with protocol version "2024-11-05"
    Then the response has JSON-RPC version "2.0"
    And the response has the same request id
    And the result contains protocol version "2024-11-05"
    And the result contains server info with name "anvil-mcp"
    And the result contains capabilities with tools enabled
