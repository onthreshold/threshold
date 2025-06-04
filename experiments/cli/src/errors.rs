use derive_more::Display;

#[derive(Debug, Display)]
pub enum KeygenError {
    #[display("Password mismatch")]
    PasswordMismatch,

    #[display("Failed to create directory.")]
    DirectoryCreation(String),

    #[display("Io error: {}", _0)]
    Io(std::io::Error),

    #[display("Failed to encode key.")]
    KeyEncoding(String),

    #[display("Failed to encrypt key.")]
    Encryption(String),

    #[display("Failed to decode key.")]
    KeyDecoding(String),

    #[display("Failed to decrypt key.")]
    Decryption(String),

    #[display("Failed to create directory.")]
    KeyFileNotFound(String),

    #[display("Failed to Serialize config. {}", _0)]
    JsonError(serde_json::Error),

    #[display("Failed to reconstruct keypair from protobuf.")]
    IdentityError(String),
}

#[derive(Debug)]
#[allow(dead_code, clippy::enum_variant_names, clippy::large_enum_variant)]
pub enum CliError {
    KeygenError(KeygenError),
    RpcError(tonic::Status),
    NodeError,
}

impl From<KeygenError> for CliError {
    fn from(error: KeygenError) -> Self {
        CliError::KeygenError(error)
    }
}
