fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `foundry-session` is a REAL feature — it is just declared by a DIFFERENT
    // package. `kit-build/anvil-kit-engine` compiles this crate's `src/main.rs`
    // as its own `[[bin]]` with that feature on, which is how the shipped
    // engine gets the Foundry session verifier without anvil-engine ever
    // depending on the private broker client.
    //
    // From anvil-engine's own build the cfg is unknown, so rustc emitted
    // "unexpected `cfg` condition value: `foundry-session`" on every build,
    // with the help text "consider adding `foundry-session` as a feature in
    // `Cargo.toml`" — step one of re-creating the optional path dependency this
    // whole split exists to remove. A warning nobody must act on is bad; a
    // warning that instructs you to reintroduce the bug is worse.
    //
    // Declared HERE rather than in a package `[lints.rust]` table because that
    // table cannot coexist with `[lints] workspace = true` (cargo hard-errors),
    // and the alternative — hand-copying the workspace lints into the package —
    // is an unguarded duplicate that will drift.
    println!("cargo::rustc-check-cfg=cfg(feature, values(\"foundry-session\"))");

    // Emit the compiled FileDescriptorSet alongside the generated code so the
    // service's RPC surface can be counted from the ACTUAL compilation product
    // rather than from a hand-maintained list that could drift from the .proto.
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("anvil_descriptor.bin"))
        .compile_protos(&["../proto/anvil.proto"], &["../proto"])?;
    Ok(())
}
