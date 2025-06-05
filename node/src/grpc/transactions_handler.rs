use bitcoin::Address;
use crate::errors::NodeError;

pub trait WindowedConfirmedTransactionProvider {
    // Must only return transactions that are confirmed in the given range [min_height, max_height].
    // All returned transactions must have at least six confirmations (< current_chain_tip_height - 6).
    async fn get_confirmed_transactions(
        &self,
        address: Address,
        min_height: u32,
        max_height: u32,
    ) -> Result<Vec<bitcoin::Transaction>, NodeError>;
}
