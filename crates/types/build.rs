fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_well_known_types(true)
        .extern_path(".google.protobuf.Timestamp", "::prost_types::Timestamp")
        .extern_path(".google.protobuf", "::prost_types")
        .compile_protos(
            &[
                "proto/grpc.proto",
                "proto/p2p.proto",
                "proto/consensus.proto",
            ],
            &["proto"],
        )?;

    Ok(())
}
