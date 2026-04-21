use std::path::PathBuf;

use aes_gcm::aead::Aead;
use aes_gcm::Aes256Gcm;
use aes_gcm::KeyInit;
use aes_gcm::Nonce;
use argon2::Argon2;
use bip39::Language;
use bip39::Mnemonic;
use rand::RngCore;
use sha2::Digest;
use sha2::Sha256;
use tauri::Manager;
use zeroize::Zeroize;

// ── Seed file format versioning ────────────────────────────────
//
// v1 (old): salt(32) || nonce(12) || ciphertext   — SHA-256 KDF
// v2 (new): magic(4) || version(1) || salt(32) || nonce(12) || ciphertext — Argon2id KDF
//
// Magic bytes: "NPT\x00"
// The magic bytes let us detect v2 files. Any file not starting with
// "NPT\x00" is treated as v1 and auto-migrated on next unlock.

const MAGIC: &[u8; 4] = b"NPT\x00";
const VERSION_ARGON2ID: u8 = 2;
const HEADER_LEN: usize = 4 + 1; // magic + version

// Argon2id parameters — tuned for mobile devices.
// ~0.5-1s on modern phones, resistant to brute-force even for short PINs.
const ARGON2_M_COST: u32 = 65536; // 64 MB memory
const ARGON2_T_COST: u32 = 3; // 3 iterations
const ARGON2_P_COST: u32 = 1; // 1 lane (single-threaded)

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
    let m = Mnemonic::parse_in(Language::English, words).map_err(|e| {
        // bip39 crate reports word positions as 0-indexed.
        // Convert to 1-indexed for the UI ("word 16" -> "word 17").
        let msg = e.to_string();
        let fixed = match msg.find("(word ") {
            Some(start) => {
                let after = &msg[start + 6..];
                if let Some(end) = after.find(')') {
                    if let Ok(n) = after[..end].parse::<usize>() {
                        format!("{}{}{}", &msg[..start + 6], n + 1, &msg[start + 6 + end..])
                    } else {
                        msg
                    }
                } else {
                    msg
                }
            }
            None => msg,
        };
        format!("Invalid mnemonic: {}", fixed)
    })?;
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

// ── Key derivation ─────────────────────────────────────────────

/// Derive AES-256 key from PIN using Argon2id (production-grade).
fn derive_key_argon2id(pin: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
            .map_err(|e| format!("Argon2 params error: {}", e))?,
    );
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(pin.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Argon2 KDF failed: {}", e))?;
    Ok(key)
}

/// Legacy SHA-256 KDF — only used for reading old v1 files during migration.
fn derive_key_sha256_legacy(pin: &str, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(pin.as_bytes());
    hasher.update(salt);
    hasher.finalize().into()
}

// ── Encrypt / Decrypt ──────────────────────────────────────────

/// Encrypt seed entropy with PIN-derived key (Argon2id, v2 format).
/// Output: magic(4) || version(1) || salt(32) || nonce(12) || ciphertext
pub fn encrypt_seed(entropy: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);

    let mut key = derive_key_argon2id(pin, &salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init failed: {}", e))?;
    key.zeroize();

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, entropy)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let mut result = Vec::with_capacity(HEADER_LEN + 32 + 12 + ciphertext.len());
    result.extend_from_slice(MAGIC);
    result.push(VERSION_ARGON2ID);
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce_bytes);
    result.extend(ciphertext);
    Ok(result)
}

/// Decrypt seed entropy. Detects format version automatically.
/// - v2 (starts with "NPT\x00"): Argon2id KDF
/// - v1 (anything else): legacy SHA-256 KDF
pub fn decrypt_seed(encrypted: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    if is_v2_format(encrypted) {
        decrypt_seed_v2(encrypted, pin)
    } else {
        decrypt_seed_v1_legacy(encrypted, pin)
    }
}

/// Check if the file uses v2 format (has our magic header).
fn is_v2_format(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN && &data[..4] == MAGIC
}

/// Decrypt v2 format: magic(4) || version(1) || salt(32) || nonce(12) || ciphertext
fn decrypt_seed_v2(encrypted: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    let min_len = HEADER_LEN + 32 + 12 + 1;
    if encrypted.len() < min_len {
        return Err("Encrypted data too short".to_string());
    }
    let salt = &encrypted[HEADER_LEN..HEADER_LEN + 32];
    let nonce_bytes = &encrypted[HEADER_LEN + 32..HEADER_LEN + 44];
    let ciphertext = &encrypted[HEADER_LEN + 44..];

    let mut key = derive_key_argon2id(pin, salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init failed: {}", e))?;
    key.zeroize();

    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Wrong PIN or corrupted data".to_string())
}

/// Decrypt v1 legacy format: salt(32) || nonce(12) || ciphertext
fn decrypt_seed_v1_legacy(encrypted: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    if encrypted.len() < 32 + 12 + 1 {
        return Err("Encrypted data too short".to_string());
    }
    let salt = &encrypted[..32];
    let nonce_bytes = &encrypted[32..44];
    let ciphertext = &encrypted[44..];

    let mut key = derive_key_sha256_legacy(pin, salt);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init failed: {}", e))?;
    key.zeroize();

    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Wrong PIN or corrupted data".to_string())
}

/// Migrate a v1 seed file to v2 format in-place.
/// Call this after successfully decrypting a v1 file.
pub fn migrate_v1_to_v2(path: &PathBuf, entropy: &[u8], pin: &str) -> Result<(), String> {
    let new_encrypted = encrypt_seed(entropy, pin)?;
    save_seed_file(path, &new_encrypted)?;
    Ok(())
}

// ── File I/O ───────────────────────────────────────────────────

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
    fn test_encrypt_decrypt_roundtrip_v2() {
        let entropy = vec![1u8; 24];
        let pin = "1234";
        let encrypted = encrypt_seed(&entropy, pin).unwrap();

        // Verify v2 header
        assert_eq!(&encrypted[..4], MAGIC);
        assert_eq!(encrypted[4], VERSION_ARGON2ID);

        let decrypted = decrypt_seed(&encrypted, pin).unwrap();
        assert_eq!(entropy, decrypted);
    }

    #[test]
    fn test_wrong_pin_v2() {
        let entropy = vec![1u8; 24];
        let encrypted = encrypt_seed(&entropy, "1234").unwrap();
        assert!(decrypt_seed(&encrypted, "5678").is_err());
    }

    #[test]
    fn test_v1_legacy_decrypt() {
        // Simulate a v1 file (no magic header, SHA-256 KDF)
        let entropy = vec![42u8; 24];
        let pin = "mypin";

        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive_key_sha256_legacy(pin, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, entropy.as_slice()).unwrap();

        let mut v1_data = Vec::new();
        v1_data.extend_from_slice(&salt);
        v1_data.extend_from_slice(&nonce_bytes);
        v1_data.extend(ciphertext);

        // Should NOT start with magic
        assert!(!is_v2_format(&v1_data));

        // Should still decrypt via auto-detect
        let decrypted = decrypt_seed(&v1_data, pin).unwrap();
        assert_eq!(entropy, decrypted);
    }

    #[test]
    fn test_v1_wrong_pin() {
        let entropy = vec![42u8; 24];
        let pin = "correct";

        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive_key_sha256_legacy(pin, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, entropy.as_slice()).unwrap();

        let mut v1_data = Vec::new();
        v1_data.extend_from_slice(&salt);
        v1_data.extend_from_slice(&nonce_bytes);
        v1_data.extend(ciphertext);

        assert!(decrypt_seed(&v1_data, "wrong").is_err());
    }

    #[test]
    fn test_entropy_roundtrip_mnemonic() {
        let mnemonic = generate_mnemonic().unwrap();
        let entropy = mnemonic_to_entropy(&mnemonic);
        assert_eq!(entropy.len(), 24);
        let restored = Mnemonic::from_entropy(&entropy).unwrap();
        assert_eq!(mnemonic.to_string(), restored.to_string());
    }
}
