fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_well_known_types(true)
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
