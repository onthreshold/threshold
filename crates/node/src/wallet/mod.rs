// PendingSpend struct shared across node handlers
use bitcoin::{Address, PublicKey, Transaction, secp256k1::Scalar};
use types::errors::NodeError;

pub mod taproot;

pub use taproot::{TaprootWallet, TrackedUtxo};

#[async_trait::async_trait]
pub trait Wallet: Send + Sync {
    fn generate_new_address(&mut self, public_key: PublicKey, tweak: Scalar) -> Address;

    fn create_spend(
        &mut self,
        amount_sat: u64,
        estimated_fee_sat: u64,
        recipient: &Address,
        dry_run: bool,
    ) -> Result<(Transaction, [u8; 32]), NodeError>;

    fn sign(
        &mut self,
        tx: &Transaction,
        private_key: &bitcoin::PrivateKey,
        sighash: [u8; 32],
    ) -> Transaction;

    async fn refresh_utxos(&mut self, allow_unconfirmed: Option<bool>) -> Result<(), NodeError>;

    fn ingest_external_tx(&mut self, tx: &Transaction) -> Result<(), NodeError>;

    fn get_utxos(&self) -> Vec<TrackedUtxo>;

    fn add_address(&mut self, address: Address);
}

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
        Ok(Self {
            tx: raw_tx,
            user_pubkey,
            recipient_script,
            fee,
        })
    }
}
