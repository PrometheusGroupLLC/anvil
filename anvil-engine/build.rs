fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Emit the compiled FileDescriptorSet alongside the generated code so the
    // service's RPC surface can be counted from the ACTUAL compilation product
    // rather than from a hand-maintained list that could drift from the .proto.
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("anvil_descriptor.bin"))
        .compile_protos(&["../proto/anvil.proto"], &["../proto"])?;
    Ok(())
}
