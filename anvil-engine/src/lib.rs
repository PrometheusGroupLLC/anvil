pub mod proto {
    tonic::include_proto!("anvil");

    /// The compiled `FileDescriptorSet` for `proto/anvil.proto`, emitted by
    /// `build.rs`. This is the compilation product itself, so anything derived
    /// from it (the served RPC surface, for instance) can never drift from the
    /// `.proto` the engine actually speaks.
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/anvil_descriptor.bin"));
}

/// The RPC method names `AnvilService` declares, in declaration order, decoded
/// from the compiled descriptor set. Fails loud if the descriptor is missing
/// the service — an empty or defaulted list would silently pass a count check.
pub fn service_rpc_names() -> Result<Vec<String>, String> {
    use prost::Message;
    let set = prost_types::FileDescriptorSet::decode(proto::FILE_DESCRIPTOR_SET)
        .map_err(|e| format!("decode anvil descriptor set: {e}"))?;
    let service = set
        .file
        .iter()
        .flat_map(|f| f.service.iter())
        .find(|s| s.name() == "AnvilService")
        .ok_or("AnvilService is absent from the compiled descriptor set")?;
    Ok(service.method.iter().map(|m| m.name().to_string()).collect())
}

pub mod abstention_ledger;
pub mod claimed_evidence;
pub mod command_seam;
pub mod engine_flags;
pub mod kit_bearer;
pub mod panel;
pub mod kiln_router;
pub mod semantic_route;
pub mod session;
pub mod startup_hooks;
pub mod step_measurement_dispatcher;
pub mod telemetry;

/// Current gRPC wire protocol version.
///
/// BUMP RULE: Increment on ANY wire-incompatible change to the gRPC messages
/// — field renumber, field-number reuse, or wire-type change. Pure append of
/// a new field number does NOT require a bump.
///
/// EXCEPTION (v2, `BeginRequest.adopt` field 29): although `adopt` is a pure
/// APPEND (proto3 field 29), a SILENT-IGNORE by an older engine is behaviorally
/// dangerous, not merely lossy. A pre-adoption (v1) engine ignores field 29, so
/// a `begin(identifier, adopt: true)` degrades into an ordinary resume/review
/// begin with NO error — the caller believes the artifact was reset to its
/// machine's initial state and driven through governance when in fact nothing
/// happened. The wire bump turns that silent divergence into the shim's
/// `client_engine_proto_version_mismatch` refusal (equality check in
/// `ensure_engine_reachable`), so a newer shim never issues `adopt` against an
/// engine that cannot honor it.
///
/// EXCEPTION (v3, candidate register/evidence fields): these are also pure
/// appends, but a v2 engine silently ignores `register: free` and treats the
/// candidate as driven. The bump makes that semantic downgrade a handshake
/// refusal instead.
pub const WIRE_PROTO_VERSION: u32 = 3;
