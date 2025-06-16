use bitcoin::{Address, PublicKey, Transaction, secp256k1::Scalar};
use types::errors::NodeError;

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
}
