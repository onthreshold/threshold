/// Error types for DKG operations
#[derive(Debug)]
pub enum DkgError {
    FrostError(frost::Error),
    InvalidParticipant(String),
    CommunicationError(String),
}

impl std::fmt::Display for DkgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DkgError::FrostError(e) => write!(f, "FROST error: {}", e),
            DkgError::InvalidParticipant(msg) => write!(f, "Invalid participant: {}", msg),
            DkgError::CommunicationError(msg) => write!(f, "Communication error: {}", msg),
        }
    }
}

impl std::error::Error for DkgError {}

impl From<frost::Error> for DkgError {
    fn from(e: frost::Error) -> Self {
        DkgError::FrostError(e)
    }
}
