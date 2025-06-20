use prost::Message;

use crate::proto::{ProtoDecode, ProtoEncode, p2p_proto};

#[derive(Debug, Clone)]
pub enum ConsensusMessage {
    LeaderAnnouncement(LeaderAnnouncement),
    NewRound(u32),
    Vote(Vote),
}

#[derive(Debug, Clone)]
pub struct LeaderAnnouncement {
    pub leader: Vec<u8>,
    pub round: u32,
}

#[derive(Debug, Clone)]
pub struct Vote {
    pub round: u32,
    pub height: u64,
    pub block_hash: Vec<u8>,
    pub voter: Vec<u8>,
    pub vote_type: VoteType,
}

#[derive(Debug, Clone)]
pub enum VoteType {
    Prevote,
    Precommit,
}

impl ProtoEncode for ConsensusMessage {
    fn encode(&self) -> Result<Vec<u8>, String> {
        let proto_msg = match self {
            Self::LeaderAnnouncement(announcement) => {
                p2p_proto::consensus_message::Message::LeaderAnnouncement(
                    p2p_proto::LeaderAnnouncement {
                        leader: announcement.leader.clone(),
                        round: announcement.round,
                    },
                )
            }
            Self::NewRound(round) => {
                p2p_proto::consensus_message::Message::NewRound(p2p_proto::NewRound {
                    round: *round,
                })
            }
            Self::Vote(vote) => p2p_proto::consensus_message::Message::Vote(p2p_proto::Vote {
                round: vote.round,
                height: vote.height,
                block_hash: vote.block_hash.clone(),
                voter: vote.voter.clone(),
                vote_type: match vote.vote_type {
                    VoteType::Prevote => p2p_proto::VoteType::Prevote as i32,
                    VoteType::Precommit => p2p_proto::VoteType::Precommit as i32,
                },
            }),
        };

        let consensus_msg = p2p_proto::ConsensusMessage {
            message: Some(proto_msg),
        };

        let mut buf = Vec::new();
        p2p_proto::ConsensusMessage::encode(&consensus_msg, &mut buf)
            .map_err(|e| format!("Failed to encode consensus message: {e}"))?;
        Ok(buf)
    }
}

impl ProtoDecode for ConsensusMessage {
    fn decode(data: &[u8]) -> Result<Self, String> {
        let proto_msg = p2p_proto::ConsensusMessage::decode(data)
            .map_err(|e| format!("Failed to decode consensus message: {e}"))?;

        let message = proto_msg.message.ok_or("Missing message field")?;

        match message {
            p2p_proto::consensus_message::Message::LeaderAnnouncement(announcement) => {
                Ok(Self::LeaderAnnouncement(LeaderAnnouncement {
                    leader: announcement.leader,
                    round: announcement.round,
                }))
            }
            p2p_proto::consensus_message::Message::NewRound(new_round) => {
                Ok(Self::NewRound(new_round.round))
            }
            p2p_proto::consensus_message::Message::Vote(vote) => {
                let vote_type = match vote.vote_type {
                    0 => VoteType::Prevote,
                    1 => VoteType::Precommit,
                    _ => return Err("Invalid vote type".to_string()),
                };
                Ok(Self::Vote(Vote {
                    round: vote.round,
                    height: vote.height,
                    block_hash: vote.block_hash,
                    voter: vote.voter,
                    vote_type,
                }))
            }
        }
    }
}
