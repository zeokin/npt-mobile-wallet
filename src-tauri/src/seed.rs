use std::fmt;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use aes_gcm::aead::Aead;
use aes_gcm::Aes256Gcm;
use aes_gcm::KeyInit;
use aes_gcm::Nonce;
use argon2::Argon2;
use bip39::Language;
use bip39::Mnemonic;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Digest;
use sha2::Sha256;
use tauri::Manager;
use zeroize::Zeroize;
use zeroize::Zeroizing;

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

/// Why a seed operation failed. Only [`SeedError::WrongPassword`] counts as a
/// failed password attempt; the other variants are reported as what they are
/// instead of "wrong password".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeedError {
    WrongPassword,
    Corrupt(String),
    Io(String),
    Kdf(String),
}

impl fmt::Display for SeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeedError::WrongPassword => write!(f, "Wrong password"),
            SeedError::Corrupt(m) => write!(f, "Wallet file is damaged: {m}"),
            SeedError::Io(m) => write!(f, "Wallet file error: {m}"),
            SeedError::Kdf(m) => write!(f, "Key derivation failed: {m}"),
        }
    }
}

impl From<SeedError> for String {
    fn from(e: SeedError) -> Self {
        e.to_string()
    }
}

/// Generate a new 18-word BIP39 mnemonic (192 bits = 24 bytes entropy).
/// This matches neptune-core's SecretKeyMaterial size.
pub fn generate_mnemonic() -> Result<Mnemonic, String> {
    let mut entropy = Zeroizing::new([0u8; 24]);
    OsRng.fill_bytes(entropy.as_mut());
    Mnemonic::from_entropy_in(Language::English, entropy.as_ref())
        .map_err(|e| format!("Failed to generate mnemonic: {}", e))
}

/// Validate a mnemonic string (must be 18 words).
pub fn validate_mnemonic(words: &str) -> Result<Mnemonic, String> {
    let normalized = words
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let word_count = normalized.split_whitespace().count();
    if word_count != 18 {
        return Err(format!("Expected 18 words, got {}", word_count));
    }
    Mnemonic::parse_in(Language::English, &normalized).map_err(|e| {
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
    })
}

/// Convert mnemonic to 24-byte entropy (SecretKeyMaterial).
pub fn mnemonic_to_entropy(mnemonic: &Mnemonic) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(mnemonic.to_entropy())
}

/// The mnemonic words for stored entropy.
pub fn entropy_to_words(entropy: &[u8]) -> Result<Zeroizing<Vec<String>>, String> {
    let mnemonic =
        Mnemonic::from_entropy(entropy).map_err(|e| format!("Invalid entropy: {}", e))?;
    Ok(Zeroizing::new(
        mnemonic.words().map(|w| w.to_string()).collect(),
    ))
}

// ── Key derivation ─────────────────────────────────────────────

/// Derive AES-256 key from PIN using Argon2id (production-grade).
fn derive_key_argon2id(pin: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, SeedError> {
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
            .map_err(|e| SeedError::Kdf(e.to_string()))?,
    );
    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(pin.as_bytes(), salt, key.as_mut())
        .map_err(|e| SeedError::Kdf(e.to_string()))?;
    Ok(key)
}

/// Legacy SHA-256 KDF — only used for reading old v1 files during migration.
fn derive_key_sha256_legacy(pin: &str, salt: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(pin.as_bytes());
    hasher.update(salt);
    Zeroizing::new(hasher.finalize().into())
}

// ── Encrypt / Decrypt ──────────────────────────────────────────

/// Encrypt seed entropy with PIN-derived key (Argon2id, v2 format).
/// Output: magic(4) || version(1) || salt(32) || nonce(12) || ciphertext
pub fn encrypt_seed(entropy: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);

    let key = derive_key_argon2id(pin, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|e| format!("Cipher init failed: {}", e))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
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
pub fn decrypt_seed(encrypted: &[u8], pin: &str) -> Result<Zeroizing<Vec<u8>>, SeedError> {
    let entropy = if is_v2_format(encrypted) {
        decrypt_seed_v2(encrypted, pin)?
    } else {
        decrypt_seed_v1_legacy(encrypted, pin)?
    };
    if entropy.len() != 24 {
        return Err(SeedError::Corrupt(format!(
            "unexpected seed length {}",
            entropy.len()
        )));
    }
    Ok(entropy)
}

/// Check if the file uses v2 format (has our magic header).
pub fn is_v2_format(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN && &data[..4] == MAGIC
}

/// Decrypt v2 format: magic(4) || version(1) || salt(32) || nonce(12) || ciphertext
fn decrypt_seed_v2(encrypted: &[u8], pin: &str) -> Result<Zeroizing<Vec<u8>>, SeedError> {
    let min_len = HEADER_LEN + 32 + 12 + 1;
    if encrypted.len() < min_len {
        return Err(SeedError::Corrupt("file too short".to_string()));
    }
    if encrypted[4] != VERSION_ARGON2ID {
        return Err(SeedError::Corrupt(format!(
            "unsupported format version {}",
            encrypted[4]
        )));
    }
    let salt = &encrypted[HEADER_LEN..HEADER_LEN + 32];
    let nonce_bytes = &encrypted[HEADER_LEN + 32..HEADER_LEN + 44];
    let ciphertext = &encrypted[HEADER_LEN + 44..];

    let key = derive_key_argon2id(pin, salt)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|e| SeedError::Kdf(format!("cipher init failed: {e}")))?;

    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map(Zeroizing::new)
        .map_err(|_| SeedError::WrongPassword)
}

/// Decrypt v1 legacy format: salt(32) || nonce(12) || ciphertext
fn decrypt_seed_v1_legacy(encrypted: &[u8], pin: &str) -> Result<Zeroizing<Vec<u8>>, SeedError> {
    if encrypted.len() < 32 + 12 + 1 {
        return Err(SeedError::Corrupt("file too short".to_string()));
    }
    let salt = &encrypted[..32];
    let nonce_bytes = &encrypted[32..44];
    let ciphertext = &encrypted[44..];

    let key = derive_key_sha256_legacy(pin, salt);
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|e| SeedError::Kdf(format!("cipher init failed: {e}")))?;

    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map(Zeroizing::new)
        .map_err(|_| SeedError::WrongPassword)
}

/// Migrate a v1 seed file to v2 format in-place (atomically).
/// Call this after successfully decrypting a v1 file.
pub fn migrate_v1_to_v2(path: &Path, entropy: &[u8], pin: &str) -> Result<(), String> {
    let new_encrypted = encrypt_seed(entropy, pin)?;
    // Make sure the new file decrypts before it replaces the old one.
    let check = decrypt_seed(&new_encrypted, pin).map_err(String::from)?;
    if check.as_slice() != entropy {
        return Err("Seed migration self-check failed".to_string());
    }
    save_seed_file(path, &new_encrypted)
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

/// Write `data` to `path` atomically: write a temporary file next to it,
/// flush it to disk, then rename it over the target. A crash or power loss
/// leaves either the old or the new file, never a truncated one.
pub fn write_file_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid file path".to_string())?;
    let tmp = path.with_file_name(format!(".{file_name}.tmp"));
    {
        let mut f =
            std::fs::File::create(&tmp).map_err(|e| format!("Failed to write {file_name}: {e}"))?;
        f.write_all(data)
            .map_err(|e| format!("Failed to write {file_name}: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("Failed to write {file_name}: {e}"))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Failed to write {file_name}: {e}")
    })
}

/// Save encrypted seed to disk (atomically).
pub fn save_seed_file(path: &Path, data: &[u8]) -> Result<(), String> {
    write_file_atomic(path, data)
}

/// Load encrypted seed from disk.
pub fn load_seed_file(path: &Path) -> Result<Vec<u8>, SeedError> {
    std::fs::read(path).map_err(|e| SeedError::Io(e.to_string()))
}

/// Check if seed file exists.
pub fn seed_exists(path: &Path) -> bool {
    path.exists()
}

/// Delete seed file.
pub fn delete_seed_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        // Overwrite before unlinking so the ciphertext does not linger in a
        // freed block (best effort; flash storage may still keep copies).
        if let Ok(meta) = std::fs::metadata(path) {
            let mut zeros = vec![0u8; meta.len() as usize];
            let _ = std::fs::write(path, &zeros);
            zeros.zeroize();
        }
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
    fn validate_mnemonic_tolerates_case_and_spacing() {
        let m = generate_mnemonic().unwrap();
        let messy = format!("  {}  ", m.to_string().to_uppercase().replace(' ', "   "));
        assert_eq!(m, validate_mnemonic(&messy).unwrap());
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
        assert_eq!(entropy, *decrypted);
    }

    #[test]
    fn test_wrong_pin_v2() {
        let entropy = vec![1u8; 24];
        let encrypted = encrypt_seed(&entropy, "1234").unwrap();
        assert_eq!(
            Err(SeedError::WrongPassword),
            decrypt_seed(&encrypted, "5678").map(|_| ())
        );
    }

    #[test]
    fn truncated_file_is_reported_as_damaged_not_wrong_password() {
        let entropy = vec![1u8; 24];
        let encrypted = encrypt_seed(&entropy, "1234").unwrap();
        let truncated = &encrypted[..HEADER_LEN + 10];
        assert!(matches!(
            decrypt_seed(truncated, "1234"),
            Err(SeedError::Corrupt(_))
        ));
    }

    fn v1_file(entropy: &[u8], pin: &str) -> Vec<u8> {
        let mut salt = [0u8; 32];
        OsRng.fill_bytes(&mut salt);
        let key = derive_key_sha256_legacy(pin, &salt);
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).unwrap();
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, entropy).unwrap();

        let mut v1_data = Vec::new();
        v1_data.extend_from_slice(&salt);
        v1_data.extend_from_slice(&nonce_bytes);
        v1_data.extend(ciphertext);
        v1_data
    }

    #[test]
    fn test_v1_legacy_decrypt() {
        let entropy = vec![42u8; 24];
        let pin = "mypin";
        let v1_data = v1_file(&entropy, pin);

        // Should NOT start with magic
        assert!(!is_v2_format(&v1_data));

        // Should still decrypt via auto-detect
        let decrypted = decrypt_seed(&v1_data, pin).unwrap();
        assert_eq!(entropy, *decrypted);
    }

    #[test]
    fn test_v1_wrong_pin() {
        let entropy = vec![42u8; 24];
        let v1_data = v1_file(&entropy, "correct");
        assert_eq!(
            Err(SeedError::WrongPassword),
            decrypt_seed(&v1_data, "wrong").map(|_| ())
        );
    }

    #[test]
    fn migration_writes_a_v2_file_atomically() {
        let dir = std::env::temp_dir().join(format!("npt-seed-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wallet.seed");
        let entropy = vec![7u8; 24];
        std::fs::write(&path, v1_file(&entropy, "pw-123456")).unwrap();

        migrate_v1_to_v2(&path, &entropy, "pw-123456").unwrap();
        let migrated = std::fs::read(&path).unwrap();
        assert!(is_v2_format(&migrated));
        assert_eq!(entropy, *decrypt_seed(&migrated, "pw-123456").unwrap());
        assert!(!dir.join(".wallet.seed.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_entropy_roundtrip_mnemonic() {
        let mnemonic = generate_mnemonic().unwrap();
        let entropy = mnemonic_to_entropy(&mnemonic);
        assert_eq!(entropy.len(), 24);
        let words = entropy_to_words(&entropy).unwrap();
        assert_eq!(mnemonic.to_string(), words.join(" "));
    }
}
