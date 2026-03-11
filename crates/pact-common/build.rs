fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../../proto";
    tonic_build::configure().build_server(true).build_client(true).compile_protos(
        &[
            "pact/config.proto",
            "pact/shell.proto",
            "pact/capability.proto",
            "pact/policy.proto",
            "pact/stream.proto",
        ],
        &[proto_root],
    )?;
    Ok(())
}
