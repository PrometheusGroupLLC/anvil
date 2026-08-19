//! anvil-mcp-steps
//!
//! Step modules used by exactly ONE brine runner (anvil-mcp). Split out of
//! anvil-test-support so no single crate carries every step definition: rustc holds
//! a whole crate's IR at once, so the largest crate sets peak build memory.
//! Shared modules stay in `anvil_test_support`.

pub mod backlog_item_mcp;
pub mod kit_build_script;
pub mod kit_manifest;
pub mod kit_playbook_source;
pub mod mcp;
pub mod mcp_claimed_evidence;
pub mod orchestrate;
