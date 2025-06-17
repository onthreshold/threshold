use prost::Message;

use crate::proto::p2p_proto;

#[derive(Debug, Clone)]
pub enum ConsensusMessage {
    LeaderAnnouncement(LeaderAnnouncement),
    NewRound(u32),
}

#[derive(Debug, Clone)]
pub struct LeaderAnnouncement {
    pub leader: Vec<u8>,
    pub round: u32,
}

impl ConsensusMessage {
    pub fn encode(msg: &Self) -> Result<Vec<u8>, String> {
        let proto_msg = match msg {
            ConsensusMessage::LeaderAnnouncement(announcement) => {
                p2p_proto::consensus_message::Message::LeaderAnnouncement(
                    p2p_proto::LeaderAnnouncement {
                        leader: announcement.leader.clone(),
                        round: announcement.round,
                    },
                )
            }
            ConsensusMessage::NewRound(round) => {
                p2p_proto::consensus_message::Message::NewRound(p2p_proto::NewRound {
                    round: *round,
                })
            }
        };

        let consensus_msg = p2p_proto::ConsensusMessage {
            message: Some(proto_msg),
        };

        let mut buf = Vec::new();
        p2p_proto::ConsensusMessage::encode(&consensus_msg, &mut buf)
            .map_err(|e| format!("Failed to encode consensus message: {}", e))?;
        Ok(buf)
    }

    pub fn decode(data: &[u8]) -> Result<Self, String> {
        let proto_msg = p2p_proto::ConsensusMessage::decode(data)
            .map_err(|e| format!("Failed to decode consensus message: {}", e))?;

        let message = proto_msg.message.ok_or("Missing message field")?;

        match message {
            p2p_proto::consensus_message::Message::LeaderAnnouncement(announcement) => {
                Ok(ConsensusMessage::LeaderAnnouncement(LeaderAnnouncement {
                    leader: announcement.leader,
                    round: announcement.round,
                }))
            }
            p2p_proto::consensus_message::Message::NewRound(new_round) => {
                Ok(ConsensusMessage::NewRound(new_round.round))
            }
        }
    }
}
