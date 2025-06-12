use bitcoin::PublicKey;
use bitcoin::secp256k1::{Secp256k1, SecretKey, rand::thread_rng};

pub fn random_public_key() -> PublicKey {
    let secp = Secp256k1::new();
    let secret_key = SecretKey::new(&mut thread_rng());
    let pk = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
    bitcoin::PublicKey::from(pk)
}
