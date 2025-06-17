pub mod p2p_proto {
    tonic::include_proto!("p2p");
}

// Include the generated proto code
pub mod node_proto {
    tonic::include_proto!("grpc");
}

use p2p_proto::direct_message::Message;

impl From<crate::network_event::DirectMessage> for p2p_proto::DirectMessage {
    fn from(msg: crate::network_event::DirectMessage) -> Self {
        let message = match msg {
            crate::network_event::DirectMessage::Ping(ping_body) => {
                Message::Ping(p2p_proto::PingMessage {
                    message: ping_body.message,
                })
            }
            crate::network_event::DirectMessage::Pong => Message::Pong(p2p_proto::PongMessage {}),
            crate::network_event::DirectMessage::Round2Package(package) => {
                let serialized =
                    serde_json::to_vec(&package).expect("Failed to serialize round2 package");
                Message::Round2Package(p2p_proto::Round2Package {
                    package_data: serialized,
                })
            }
            crate::network_event::DirectMessage::SignRequest { sign_id, message } => {
                Message::SignRequest(p2p_proto::SignRequest { sign_id, message })
            }
            crate::network_event::DirectMessage::SignPackage { sign_id, package } => {
                Message::SignPackage(p2p_proto::SignPackage { sign_id, package })
            }
            crate::network_event::DirectMessage::Commitments {
                sign_id,
                commitments,
            } => Message::Commitments(p2p_proto::Commitments {
                sign_id,
                commitments,
            }),
            crate::network_event::DirectMessage::SignatureShare {
                sign_id,
                signature_share,
            } => Message::SignatureShare(p2p_proto::SignatureShare {
                sign_id,
                signature_share,
            }),
        };

        Self {
            message: Some(message),
        }
    }
}

impl TryFrom<p2p_proto::DirectMessage> for crate::network_event::DirectMessage {
    type Error = String;

    fn try_from(proto_msg: p2p_proto::DirectMessage) -> Result<Self, Self::Error> {
        let message = proto_msg.message.ok_or("Missing message field")?;

        match message {
            Message::Ping(ping) => Ok(Self::Ping(crate::network_event::PingBody {
                message: ping.message,
            })),
            Message::Pong(_) => Ok(Self::Pong),
            Message::Round2Package(package) => {
                let round2_package = serde_json::from_slice(&package.package_data)
                    .map_err(|e| format!("Failed to deserialize round2 package: {e}"))?;
                Ok(Self::Round2Package(round2_package))
            }
            Message::SignRequest(req) => Ok(Self::SignRequest {
                sign_id: req.sign_id,
                message: req.message,
            }),
            Message::SignPackage(pkg) => Ok(Self::SignPackage {
                sign_id: pkg.sign_id,
                package: pkg.package,
            }),
            Message::Commitments(comm) => Ok(Self::Commitments {
                sign_id: comm.sign_id,
                commitments: comm.commitments,
            }),
            Message::SignatureShare(share) => Ok(Self::SignatureShare {
                sign_id: share.sign_id,
                signature_share: share.signature_share,
            }),
        }
    }
}
