use crate::{
    consensus::ConsensusMessage,
    intents::{DepositIntent, PendingSpend},
    proto::{ProtoDecode, ProtoEncode, p2p_proto},
};

use prost::Message as _;

#[derive(Debug, Clone)]
pub enum BroadcastMessage {
    /// Messages related to tendermint consensus
    Consensus(ConsensusMessage),
    /// Message containing a proposed block that needs to be validated by peers.
    Block(Vec<u8>),
    /// Message containing a deposit intent that needs to be validated by peers.
    DepositIntent(DepositIntent),
    /// Message containing a fully signed withdrawal transaction that should be broadcast to the Bitcoin network and accounted locally.
    PendingSpend(PendingSpend),
    /// Message containing a Frost DKG coordination message.
    Dkg(p2p_proto::GossipsubMessage),
}

const CONSENSUS_TAG: u8 = 0;
const BLOCK_TAG: u8 = 1;
const DEPOSIT_INTENT_TAG: u8 = 2;
const PENDING_SPEND_TAG: u8 = 3;
const DKG_TAG: u8 = 4;

impl ProtoEncode for BroadcastMessage {
    fn encode(&self) -> Result<Vec<u8>, String> {
        let mut buf = Vec::with_capacity(1 + 128); // 1 byte tag + payload (size heuristic)
        match self {
            Self::Consensus(msg) => {
                buf.push(CONSENSUS_TAG);
                buf.extend(msg.encode()?);
            }
            Self::Block(raw) => {
                buf.push(BLOCK_TAG);
                buf.extend(raw);
            }
            Self::DepositIntent(intent) => {
                buf.push(DEPOSIT_INTENT_TAG);
                buf.extend(intent.encode()?);
            }
            Self::PendingSpend(spend) => {
                buf.push(PENDING_SPEND_TAG);
                buf.extend(spend.encode()?);
            }
            Self::Dkg(msg) => {
                buf.push(DKG_TAG);
                buf.extend(msg.encode_to_vec());
            }
        }
        Ok(buf)
    }
}

impl ProtoDecode for BroadcastMessage {
    fn decode(data: &[u8]) -> Result<Self, String> {
        if data.is_empty() {
            return Err("Empty broadcast message".to_string());
        }

        let (tag, payload) = data.split_first().expect("checked above");

        match *tag {
            CONSENSUS_TAG => Ok(Self::Consensus(ConsensusMessage::decode(payload)?)),
            BLOCK_TAG => Ok(Self::Block(payload.to_vec())),
            DEPOSIT_INTENT_TAG => Ok(Self::DepositIntent(DepositIntent::decode(payload)?)),
            PENDING_SPEND_TAG => Ok(Self::PendingSpend(PendingSpend::decode(payload)?)),
            DKG_TAG => Ok(Self::Dkg(
                p2p_proto::GossipsubMessage::decode(payload).map_err(|e| e.to_string())?,
            )),
            other => Err(format!("Unknown BroadcastMessage tag: {other}")),
        }
    }
}
