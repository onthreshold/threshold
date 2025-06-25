use bincode::de::{BorrowDecode, BorrowDecoder};
use bincode::{Decode, Encode, de::Decoder, enc::Encoder};
use bitcoin::hashes::Hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use types::errors::NodeError;

pub type TransactionId = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub version: u32,
    pub r#type: TransactionType,
    pub operations: Vec<Operation>,
    pub metadata: Option<serde_json::Value>,
}

impl Encode for Transaction {
    fn encode<E: Encoder>(&self, e: &mut E) -> Result<(), bincode::error::EncodeError> {
        self.version.encode(e)?;
        self.r#type.encode(e)?;
        self.operations.encode(e)?;
        let metadata_string = serde_json::to_string(&self.metadata).unwrap();
        metadata_string.encode(e)?;
        Ok(())
    }
}

impl<C> Decode<C> for Transaction {
    fn decode<D: Decoder>(d: &mut D) -> Result<Self, bincode::error::DecodeError> {
        let version = u32::decode(d)?;
        let r#type = TransactionType::decode(d)?;
        let operations = Vec::<Operation>::decode(d)?;
        let metadata = String::decode(d)?;
        let metadata = serde_json::from_str(&metadata).unwrap();
        Ok(Self {
            version,
            r#type,
            operations,
            metadata,
        })
    }
}

impl<'de, C> BorrowDecode<'de, C> for Transaction {
    fn borrow_decode<D: BorrowDecoder<'de>>(
        d: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        Self::decode(d)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub enum TransactionType {
    Deposit,
    Withdrawal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
pub enum Operation {
    /// Push a value to the stack in bytes
    /// Data types:
    ///    - Numbers: u64
    ///    - Strings: utf-8 encoded string
    ///    - Booleans: u8 (0 or 1)
    ///    - Tx Hash: [u8; 32]
    OpPush { value: Vec<u8> },
    /// Check if the transaction is on the Bitcoin network. Modifies allowance list to allow the address to spend the amount.
    /// Pops from the stack:
    ///   - 0: The tx hash
    ///   - 1: The address
    ///   - 2: The amount
    ///
    /// Pushes to the stack:
    ///   - 0: The result (0 or 1)
    OpCheckOracle,
    /// Increment the balance of the address on the stack. Checks the allowance list to see if the address is allowed to spend the amount.
    /// Pops from the stack:
    ///   - 0: The address
    ///   - 1: The amount
    ///
    /// Pushes to the stack:
    ///   - 0: The result (0 or 1)
    OpIncrementBalance,
    /// Decrement the balance of the address on the stack.
    /// Pops from the stack:
    ///   - 0: The address
    ///   - 1: The amount
    ///
    /// Pushes to the stack:
    ///   - 0: The result (0 or 1)
    OpDecrementBalance,
}

impl Transaction {
    #[must_use]
    pub const fn new(
        r#type: TransactionType,
        operations: Vec<Operation>,
        metadata: Option<serde_json::Value>,
    ) -> Self {
        Self {
            version: 1,
            r#type,
            operations,
            metadata,
        }
    }

    #[must_use]
    pub fn id(&self) -> TransactionId {
        let mut hasher = Sha256::new();
        hasher.update(self.version.to_le_bytes());

        // Include transaction type in hash for deterministic sorting
        let type_bytes = bincode::encode_to_vec(&self.r#type, bincode::config::standard()).unwrap();
        hasher.update(&type_bytes);

        for op in &self.operations {
            let op_bytes = bincode::encode_to_vec(op, bincode::config::standard()).unwrap();
            hasher.update(&op_bytes);
        }

        if let Some(metadata) = &self.metadata {
            let metadata_string = serde_json::to_string(metadata).unwrap();
            hasher.update(metadata_string.as_bytes());
        }

        let result = hasher.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&result);
        id
    }

    pub fn create_deposit_transaction(
        tx: &bitcoin::Transaction,
        user_pubkey: &str,
        amount_sat: u64,
    ) -> Result<Self, NodeError> {
        Ok(Self::new(
            TransactionType::Deposit,
            vec![
                Operation::OpPush {
                    value: amount_sat.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: user_pubkey.as_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: tx.compute_txid().as_byte_array().to_vec(),
                },
                Operation::OpCheckOracle,
                Operation::OpPush {
                    value: amount_sat.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: user_pubkey.as_bytes().to_vec(),
                },
                Operation::OpIncrementBalance,
            ],
            Some(serde_json::json!({
                "tx": tx,
                "user_pubkey": user_pubkey,
                "amount_sat": amount_sat,
            })),
        ))
    }

    pub fn get_deposit_transaction_address(&self) -> Result<bitcoin::Transaction, NodeError> {
        let tx = self
            .metadata
            .as_ref()
            .ok_or_else(|| NodeError::Error("No metadata".to_string()))?;
        let tx = serde_json::from_value(
            tx.get("tx")
                .ok_or_else(|| NodeError::Error("No tx".to_string()))?
                .clone(),
        )
        .map_err(|_| NodeError::Error("Invalid tx".to_string()))?;

        Ok(tx)
    }

    pub fn create_withdrawal_transaction(
        user_pubkey: &str,
        address_to: &str,
        amount_sat: u64,
    ) -> Result<Self, NodeError> {
        Ok(Self::new(
            TransactionType::Withdrawal,
            vec![
                Operation::OpPush {
                    value: amount_sat.to_be_bytes().to_vec(),
                },
                Operation::OpPush {
                    value: user_pubkey.as_bytes().to_vec(),
                },
                Operation::OpDecrementBalance,
            ],
            Some(serde_json::json!({
                "user_pubkey": user_pubkey,
                "amount_sat": amount_sat,
                "address_to": address_to,
            })),
        ))
    }
}
