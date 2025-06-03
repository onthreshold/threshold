use super::*;
use assert_matches::assert_matches;
use std::fs;
use tempfile::tempdir;

// Helper function to decrypt private key
fn decrypt_private_key(
    encrypted_key: &str,
    password: &str,
    params: &EncryptionParams,
) -> Result<Vec<u8>, KeygenError> {
    let salt = SaltString::from_b64(&params.salt_b64)
        .map_err(|e| KeygenError::Encryption(e.to_string()))?;
    let key = generate_key(password, &salt)?;

    let nonce_bytes = BASE64
        .decode(&params.iv_b64)
        .map_err(|e| KeygenError::Encryption(e.to_string()))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));

    let ciphertext = BASE64
        .decode(encrypted_key)
        .map_err(|e| KeygenError::Encryption(e.to_string()))?;

    cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| KeygenError::Encryption(e.to_string()))
}

// Test helper to generate keypair without password prompt
fn generate_test_keypair(output_path: Option<PathBuf>, password: &str) -> Result<(), KeygenError> {
    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public().encode_protobuf();
    let public_key_b58 = bs58::encode(public_key).into_string();

    let (encrypted_private_key, encryption_params) = encrypt_private_key(&keypair, password)?;

    let key_data = KeyData {
        public_key_b58: public_key_b58.clone(),
        encrypted_private_key_b64: encrypted_private_key,
        encryption_params,
    };

    let json = serde_json::to_string_pretty(&key_data)
        .map_err(|e| KeygenError::Io(std::io::Error::other(e)))?;

    let key_file_path = if let Some(path) = output_path {
        if path.is_dir() {
            path.join("config.json")
        } else {
            path
        }
    } else {
        get_key_file_path()?
    };

    fs::write(&key_file_path, json).map_err(KeygenError::Io)?;
    Ok(())
}

#[test]
fn test_key_generation_and_encryption() {
    let keypair = Keypair::generate_ed25519();
    let password = "test_password123";

    let (encrypted_key, params) = encrypt_private_key(&keypair, password).unwrap();

    assert_eq!(params.kdf, "argon2id");
    assert!(!params.salt_b64.is_empty());
    assert!(!params.iv_b64.is_empty());
    assert!(!encrypted_key.is_empty());

    // Verify we can decrypt with correct password
    let decrypted = decrypt_private_key(&encrypted_key, password, &params).unwrap();
    let original = keypair.to_protobuf_encoding().unwrap();
    assert_eq!(decrypted, original);
}

#[test]
fn test_decryption_with_wrong_password() {
    let keypair = Keypair::generate_ed25519();
    let password = "correct_password";
    let wrong_password = "wrong_password";

    let (encrypted_key, params) = encrypt_private_key(&keypair, password).unwrap();

    // Attempt decryption with wrong password should fail
    let result = decrypt_private_key(&encrypted_key, wrong_password, &params);
    assert!(result.is_err());
}

#[test]
fn test_key_file_operations() {
    let temp_dir = tempdir().unwrap();
    let output_path = temp_dir.path().join("test_config.json");

    // Test key generation and file writing
    let result = generate_test_keypair(Some(output_path.clone()), "test_password123");
    assert!(result.is_ok());

    // Verify file exists and contains valid JSON
    let contents = fs::read_to_string(&output_path).unwrap();
    let key_data: KeyData = serde_json::from_str(&contents).unwrap();

    assert!(!key_data.public_key_b58.is_empty());
    assert!(!key_data.encrypted_private_key_b64.is_empty());
    assert_eq!(key_data.encryption_params.kdf, "argon2id");
}

#[test]
fn test_invalid_directory() {
    // Test with a non-existent directory
    let result = generate_test_keypair(
        Some(PathBuf::from("/nonexistent/path/config.json")),
        "test_password123",
    );
    assert_matches!(result, Err(KeygenError::Io(_)));
}

#[test]
fn test_key_encoding() {
    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public().encode_protobuf();
    let public_key_b58 = bs58::encode(public_key).into_string();

    // Verify the public key is properly encoded
    assert!(!public_key_b58.is_empty());
    assert!(bs58::decode(&public_key_b58).into_vec().is_ok());
}

#[test]
fn test_encryption_params_serialization() {
    let params = EncryptionParams {
        kdf: "argon2id".to_string(),
        salt_b64: "test_salt".to_string(),
        iv_b64: "test_iv".to_string(),
    };

    let json = serde_json::to_string(&params).unwrap();
    let deserialized: EncryptionParams = serde_json::from_str(&json).unwrap();

    assert_eq!(params.kdf, deserialized.kdf);
    assert_eq!(params.salt_b64, deserialized.salt_b64);
    assert_eq!(params.iv_b64, deserialized.iv_b64);
}
