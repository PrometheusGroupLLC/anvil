Feature: JSON-RPC error code conformance
  The MCP shim returns canonical JSON-RPC 2.0 error codes and stable
  string discriminators in error.data.code for protocol-level errors.

  Scenario: Unknown method returns method_not_found error
    Given the MCP shim is started
    When an unknown method request is sent
    Then the response error code is -32601
    And the response error data code is "method_not_found"

  Scenario: Missing required tools/call parameter returns invalid_params error
    Given the MCP shim is started
    When a tools/call request with no name field is sent
    Then the response error code is -32602
    And the response error data code is "invalid_params"
