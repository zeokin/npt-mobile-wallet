use bip39::{Language, Mnemonic};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use rand::RngCore;
use sha2::{Sha256, Digest};
use tauri::Manager;
use zeroize::Zeroize;
use std::path::PathBuf;

/// Generate a new 18-word BIP39 mnemonic (192 bits = 24 bytes entropy).
/// This matches neptune-core's SecretKeyMaterial size.
pub fn generate_mnemonic() -> Result<Mnemonic, String> {
    let mut entropy = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut entropy);
    let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
        .map_err(|e| format!("Failed to generate mnemonic: {}", e))?;
    entropy.zeroize();
    Ok(mnemonic)
}

/// Validate a mnemonic string (must be 18 words).
pub fn validate_mnemonic(words: &str) -> Result<Mnemonic, String> {
    let m = Mnemonic::parse_in(Language::English, words)
        .map_err(|e| format!("Invalid mnemonic: {}", e))?;
    let word_count = words.split_whitespace().count();
    if word_count != 18 {
        return Err(format!("Expected 18 words, got {}", word_count));
    }
    Ok(m)
}

/// Convert mnemonic to 24-byte entropy (SecretKeyMaterial).
pub fn mnemonic_to_entropy(mnemonic: &Mnemonic) -> Vec<u8> {
    mnemonic.to_entropy()
}

/// Derive AES-256 key from PIN using SHA-256.
/// TODO: Replace with Argon2id for production.
fn derive_key(pin: &str, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(pin.as_bytes());
    hasher.update(salt);
    hasher.finalize().into()
}

/// Encrypt seed entropy with PIN-derived key.
/// Format: salt (32 bytes) || nonce (12 bytes) || ciphertext
pub fn encrypt_seed(entropy: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut key = derive_key(pin, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    key.zeroize();

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, entropy)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut result = Vec::with_capacity(32 + 12 + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce_bytes);
    result.extend(ciphertext);
    Ok(result)
}

/// Decrypt seed entropy with PIN.
pub fn decrypt_seed(encrypted: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    if encrypted.len() < 32 + 12 + 1 {
        return Err("Encrypted data too short".to_string());
    }
    let salt = &encrypted[..32];
    let nonce_bytes = &encrypted[32..44];
    let ciphertext = &encrypted[44..];

    let mut key = derive_key(pin, salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    key.zeroize();

    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Wrong PIN or corrupted data".to_string())
}

/// Get the path for the seed file (Tauri v2 API).
pub fn seed_file_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create dir: {}", e))?;
    Ok(dir.join("wallet.seed"))
}

/// Save encrypted seed to disk.
pub fn save_seed_file(path: &PathBuf, data: &[u8]) -> Result<(), String> {
    std::fs::write(path, data).map_err(|e| format!("Failed to write seed file: {}", e))
}

/// Load encrypted seed from disk.
pub fn load_seed_file(path: &PathBuf) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("Failed to read seed file: {}", e))
}

/// Check if seed file exists.
pub fn seed_exists(path: &PathBuf) -> bool {
    path.exists()
}

/// Delete seed file.
pub fn delete_seed_file(path: &PathBuf) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("Failed to delete seed file: {}", e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate() {
        let m = generate_mnemonic().unwrap();
        let words = m.to_string();
        let word_count = words.split_whitespace().count();
        assert_eq!(word_count, 18);
        validate_mnemonic(&words).unwrap();
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let entropy = vec![1u8; 24];
        let pin = "1234";
        let encrypted = encrypt_seed(&entropy, pin).unwrap();
        let decrypted = decrypt_seed(&encrypted, pin).unwrap();
        assert_eq!(entropy, decrypted);
    }

    #[test]
    fn test_wrong_pin() {
        let entropy = vec![1u8; 24];
        let encrypted = encrypt_seed(&entropy, "1234").unwrap();
        assert!(decrypt_seed(&encrypted, "5678").is_err());
    }
}
