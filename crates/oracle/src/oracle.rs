use bitcoin::Transaction;
use bitcoin::{Address, Txid};
use dyn_clone::DynClone;
use types::{errors::NodeError, utxo::Utxo};

#[async_trait::async_trait]
pub trait Oracle: Send + DynClone + Sync {
    async fn validate_transaction(
        &self,
        address: &str,
        amount: u64,
        tx_hash: Txid,
    ) -> Result<bool, NodeError>;

    async fn get_transaction_by_address(&self, tx_id: &str) -> Result<Transaction, NodeError>;

    async fn get_current_fee_per_vb(&self, priority: Option<u16>) -> Result<f64, NodeError>;
    async fn refresh_utxos(
        &self,
        address: Address,
        number_pages: u32,
        start_transactions: Option<Txid>,
        allow_unconfirmed: bool,
    ) -> Result<Vec<Utxo>, NodeError>;

    async fn broadcast_transaction(&self, tx: &bitcoin::Transaction) -> Result<String, NodeError>;

    async fn get_confirmed_transactions(
        &self,
        addresses: Vec<Address>,
        min_height: u32,
        max_height: u32,
    ) -> Result<Vec<Transaction>, NodeError>;

    async fn poll_new_transactions(&mut self, addresses: Vec<Address>);

    async fn get_latest_block_height(&self) -> Result<u32, NodeError>;
}

dyn_clone::clone_trait_object!(Oracle);
