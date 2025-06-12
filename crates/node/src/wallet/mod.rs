// PendingSpend struct shared across node handlers
use bincode::{Decode, Encode};
use bitcoin::{ScriptBuf, Transaction};

pub mod taproot;
pub mod traits;

pub use taproot::{TaprootWallet, TrackedUtxo};
pub use traits::Wallet;

#[derive(Debug)]
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
        Ok(PendingSpend {
            tx: raw_tx,
            user_pubkey,
            recipient_script,
            fee,
        })
    }
}
