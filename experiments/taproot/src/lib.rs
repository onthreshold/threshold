//! # FROST Taproot Wallet
//!
//! A production-ready implementation of FROST (Flexible Round-Optimized Schnorr Threshold signatures)
//! integrated with Bitcoin Taproot for secure multi-signature wallets.
//!
//! ## Features
//!
//! - **Distributed Key Generation (DKG)**: No trusted dealer required
//! - **Taproot Integration**: Efficient key-path spending for privacy and cost savings
//! - **Threshold Signatures**: Configurable m-of-n signing schemes
//! - **Production Ready**: Comprehensive error handling and validation

pub mod dkg;
pub mod utils;
pub mod wallet;

#[cfg(test)]
mod tests;

// Re-export main types for convenience
pub use dkg::{DkgResult, perform_distributed_key_generation};
pub use utils::{create_mock_transaction, create_utxo};
pub use wallet::{FrostTaprootWallet, Utxo};
