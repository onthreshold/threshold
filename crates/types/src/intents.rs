use bincode::{Decode, Encode};
use bitcoin::{ScriptBuf, Transaction};
use prost::Message;
use serde::{Deserialize, Serialize};

use crate::proto::{ProtoDecode, ProtoEncode, p2p_proto};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct DepositIntent {
    pub amount_sat: u64,
    pub user_pubkey: String,
    pub deposit_tracking_id: String,
    pub deposit_address: String,
    pub timestamp: u64,
}

impl ProtoEncode for DepositIntent {
    fn encode(&self) -> Result<Vec<u8>, String> {
        let proto_intent = p2p_proto::DepositIntent {
            amount_sat: self.amount_sat,
            user_pubkey: self.user_pubkey.clone(),
            deposit_tracking_id: self.deposit_tracking_id.clone(),
            deposit_address: self.deposit_address.clone(),
            timestamp: self.timestamp,
        };

        let mut buf = Vec::new();
        p2p_proto::DepositIntent::encode(&proto_intent, &mut buf)
            .map_err(|e| format!("Failed to encode deposit intent: {e}"))?;
        Ok(buf)
    }
}

impl ProtoDecode for DepositIntent {
    fn decode(data: &[u8]) -> Result<Self, String> {
        let proto_intent = p2p_proto::DepositIntent::decode(data)
            .map_err(|e| format!("Failed to decode deposit intent: {e}"))?;

        Ok(Self {
            amount_sat: proto_intent.amount_sat,
            user_pubkey: proto_intent.user_pubkey,
            deposit_tracking_id: proto_intent.deposit_tracking_id,
            deposit_address: proto_intent.deposit_address,
            timestamp: proto_intent.timestamp,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawlIntent {
    pub amount_sat: u64,
    pub address_to: String,
    pub public_key: String,
    pub blocks_to_confirm: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct PendingSpend {
    pub tx: Transaction,
    pub user_pubkey: String,
    pub recipient_script: ScriptBuf,
    pub fee: u64,
}

impl Encode for PendingSpend {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        let raw_tx = bitcoin::consensus::encode::serialize(&self.tx);
        bincode::Encode::encode(&raw_tx, encoder)?;
        bincode::Encode::encode(&self.user_pubkey, encoder)?;
        bincode::Encode::encode(&self.recipient_script.as_bytes(), encoder)?;
        bincode::Encode::encode(&self.fee, encoder)?;
        Ok(())
    }
}

impl<Context> Decode<Context> for PendingSpend {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let raw_tx_bytes: Vec<u8> = bincode::Decode::decode(decoder)?;
        let raw_tx: Transaction = bitcoin::consensus::encode::deserialize(&raw_tx_bytes)
            .map_err(|_| bincode::error::DecodeError::Other("Failed to deserialize transaction"))?;
        let user_pubkey = bincode::Decode::decode(decoder)?;
        let recipient_script = ScriptBuf::from_bytes(bincode::Decode::decode(decoder)?);
        let fee = bincode::Decode::decode(decoder)?;
        Ok(Self {
            tx: raw_tx,
            user_pubkey,
            recipient_script,
            fee,
        })
    }
}

impl ProtoEncode for PendingSpend {
    fn encode(&self) -> Result<Vec<u8>, String> {
        let transaction_bytes = bitcoin::consensus::encode::serialize(&self.tx);
        let script_bytes = self.recipient_script.to_bytes();

        let proto_intent = p2p_proto::PendingSpend {
            transaction: transaction_bytes,
            user_pubkey: self.user_pubkey.clone(),
            recipient_script: script_bytes,
            fee: self.fee,
        };

        let mut buf = Vec::new();
        p2p_proto::PendingSpend::encode(&proto_intent, &mut buf)
            .map_err(|e| format!("Failed to encode pending spend: {e}"))?;
        Ok(buf)
    }
}

impl ProtoDecode for PendingSpend {
    fn decode(data: &[u8]) -> Result<Self, String> {
        let proto_intent = p2p_proto::PendingSpend::decode(data)
            .map_err(|e| format!("Failed to decode pending spend: {e}"))?;

        let tx = bitcoin::consensus::encode::deserialize(&proto_intent.transaction)
            .map_err(|e| format!("Failed to deserialize transaction: {e}"))?;

        let recipient_script = bitcoin::ScriptBuf::from_bytes(proto_intent.recipient_script);

        Ok(Self {
            tx,
            user_pubkey: proto_intent.user_pubkey,
            recipient_script,
            fee: proto_intent.fee,
        })
    }
}
