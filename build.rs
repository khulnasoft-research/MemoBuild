fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::var("OUT_DIR")?;

    // Use protobuf-src to provide protoc
    std::env::set_var("PROTOC", protobuf_src::protoc());

    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(format!("{}/memobuild_v1_descriptor.bin", out_dir))
        .compile_protos(&["proto/memobuild/v1/execution.proto"], &["proto"])?;

    Ok(())
}
