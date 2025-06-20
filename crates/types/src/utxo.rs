use bincode::{Decode, Encode};
use bitcoin::{Amount, OutPoint, ScriptBuf};

#[derive(Debug, Clone)]
pub struct Utxo {
    pub outpoint: OutPoint,
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
}

impl Encode for Utxo {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        let outpoint_bytes = bitcoin::consensus::encode::serialize(&self.outpoint);
        let value_sat = self.value.to_sat();
        let script_bytes = self.script_pubkey.as_bytes();

        bincode::Encode::encode(&outpoint_bytes, encoder)?;
        bincode::Encode::encode(&value_sat, encoder)?;
        bincode::Encode::encode(&script_bytes, encoder)?;
        Ok(())
    }
}

impl<Context> Decode<Context> for Utxo {
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let outpoint_bytes: Vec<u8> = bincode::Decode::decode(decoder)?;
        let value_sat: u64 = bincode::Decode::decode(decoder)?;
        let script_bytes: Vec<u8> = bincode::Decode::decode(decoder)?;

        let outpoint: OutPoint = bitcoin::consensus::encode::deserialize(&outpoint_bytes)
            .map_err(|_| bincode::error::DecodeError::Other("Failed to deserialize outpoint"))?;
        let value = Amount::from_sat(value_sat);
        let script_pubkey = ScriptBuf::from_bytes(script_bytes);

        Ok(Self {
            outpoint,
            value,
            script_pubkey,
        })
    }
}
