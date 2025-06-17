fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(&["proto/grpc.proto", "proto/p2p.proto"], &["proto"])?;

    Ok(())
}
