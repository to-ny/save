fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_file = "../../proto/replication.proto";

    // Use protox for pure-Rust protobuf compilation (no system protoc required)
    let file_descriptor_set = protox::compile([proto_file], ["../../proto"])?;

    // Generate Rust code with prost
    prost_build::Config::new().compile_fds(file_descriptor_set)?;

    println!("cargo:rerun-if-changed={}", proto_file);

    Ok(())
}
