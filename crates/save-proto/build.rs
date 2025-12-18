fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_files = [
        "proto/replication.proto",
        "proto/raft.proto",
        "proto/cluster.proto",
    ];

    // Use protox for pure-Rust protobuf compilation (no system protoc required)
    let file_descriptor_set = protox::compile(proto_files, ["proto"])?;

    // Generate Rust types from protobuf
    prost_build::Config::new().compile_fds(file_descriptor_set)?;

    for proto_file in &proto_files {
        println!("cargo:rerun-if-changed={}", proto_file);
    }

    Ok(())
}
