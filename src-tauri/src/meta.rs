//! Per-wallet metadata kept on the device: how many receiving addresses of
//! each type have been handed out ("New address"). Every handed-out address
//! is scanned and listed, and indices at which funds were found count as
//! handed out (like the desktop wallet).
//!
//! Survives a rescan (unlike the scan cache); removed when the wallet is
//! replaced or deleted.

use std::path::Path;
use std::path::PathBuf;

use neptune_wallet::address::KeyType;
use serde::Deserialize;
use serde::Serialize;

use crate::sync::ScanWindow;

const META_FILE: &str = "wallet_meta.json";
const META_VERSION: u32 = 1;

/// Upper bound of handed-out addresses per type.
pub(crate) const MAX_ISSUED_ADDRESSES: u64 = 1_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WalletMeta {
    version: u32,
    /// Next index to hand out, per type. Index 0 is the main address, so the
    /// minimum is 1.
    generation: u64,
    ec_hybrid: u64,
    viewing_address: u64,
}

impl Default for WalletMeta {
    fn default() -> Self {
        Self {
            version: META_VERSION,
            generation: 1,
            ec_hybrid: 1,
            viewing_address: 1,
        }
    }
}

impl WalletMeta {
    fn path(dir: &Path) -> PathBuf {
        dir.join(META_FILE)
    }

    pub(crate) fn load(dir: &Path) -> Self {
        let meta: Self = std::fs::read(Self::path(dir))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            version: META_VERSION,
            generation: meta.generation.clamp(1, MAX_ISSUED_ADDRESSES),
            ec_hybrid: meta.ec_hybrid.clamp(1, MAX_ISSUED_ADDRESSES),
            viewing_address: meta.viewing_address.clamp(1, MAX_ISSUED_ADDRESSES),
        }
    }

    pub(crate) fn save(&self, dir: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec(self).map_err(|e| format!("Encode wallet data: {e}"))?;
        crate::seed::write_file_atomic(&Self::path(dir), &bytes)
    }

    pub(crate) fn delete(dir: &Path) {
        let _ = std::fs::remove_file(Self::path(dir));
    }

    /// Next index to hand out for `key_type`.
    pub(crate) fn next_index(&self, key_type: KeyType) -> u64 {
        match key_type {
            KeyType::Generation => self.generation,
            KeyType::EcHybrid => self.ec_hybrid,
            KeyType::ViewingAddress => self.viewing_address,
            _ => 1,
        }
    }

    fn set_next_index(&mut self, key_type: KeyType, next: u64) {
        let next = next.clamp(1, MAX_ISSUED_ADDRESSES);
        match key_type {
            KeyType::Generation => self.generation = next,
            KeyType::EcHybrid => self.ec_hybrid = next,
            KeyType::ViewingAddress => self.viewing_address = next,
            _ => {}
        }
    }

    /// Index of the newest handed-out address of `key_type` (the one shown
    /// for receiving).
    pub(crate) fn current_index(&self, key_type: KeyType) -> u64 {
        self.next_index(key_type) - 1
    }

    /// Hand out the next address index of `key_type`.
    pub(crate) fn issue(&mut self, key_type: KeyType) -> Result<u64, String> {
        if !matches!(
            key_type,
            KeyType::Generation | KeyType::EcHybrid | KeyType::ViewingAddress
        ) {
            return Err("This key type has no shareable addresses".to_string());
        }
        let index = self.next_index(key_type);
        if index + 1 > MAX_ISSUED_ADDRESSES {
            return Err(format!(
                "At most {MAX_ISSUED_ADDRESSES} addresses per type are supported"
            ));
        }
        self.set_next_index(key_type, index + 1);
        Ok(index)
    }

    /// Count indices at which funds were found as handed out. Returns whether
    /// anything changed.
    pub(crate) fn absorb_found(&mut self, found: impl IntoIterator<Item = (KeyType, u64)>) -> bool {
        let before = *self;
        for (key_type, index) in found {
            if index + 1 > self.next_index(key_type) {
                self.set_next_index(key_type, index + 1);
            }
        }
        before != *self
    }

    /// Smallest scan window covering every handed-out address plus the usual
    /// look-ahead gap.
    pub(crate) fn scan_floor(&self) -> ScanWindow {
        let default = ScanWindow::default();
        ScanWindow {
            generation: self.generation + ScanWindow::gap(KeyType::Generation),
            ec_hybrid: self.ec_hybrid + ScanWindow::gap(KeyType::EcHybrid),
            viewing_address: self.viewing_address + ScanWindow::gap(KeyType::ViewingAddress),
            symmetric: default.symmetric,
        }
        .max(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuing_addresses_advances_and_persists() {
        let dir = std::env::temp_dir().join(format!("npt-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut meta = WalletMeta::load(&dir);
        assert_eq!(0, meta.current_index(KeyType::EcHybrid));
        assert_eq!(1, meta.issue(KeyType::EcHybrid).unwrap());
        assert_eq!(2, meta.issue(KeyType::EcHybrid).unwrap());
        assert_eq!(2, meta.current_index(KeyType::EcHybrid));
        assert_eq!(0, meta.current_index(KeyType::ViewingAddress));
        assert!(meta.issue(KeyType::Symmetric).is_err());
        meta.save(&dir).unwrap();
        assert_eq!(meta, WalletMeta::load(&dir));
        WalletMeta::delete(&dir);
        assert_eq!(WalletMeta::default(), WalletMeta::load(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn found_funds_count_as_handed_out_and_widen_the_scan() {
        let mut meta = WalletMeta::default();
        assert!(meta.absorb_found([(KeyType::EcHybrid, 6), (KeyType::Symmetric, 4)]));
        assert_eq!(6, meta.current_index(KeyType::EcHybrid));
        assert!(!meta.absorb_found([(KeyType::EcHybrid, 2)]));
        let floor = meta.scan_floor();
        assert_eq!(7 + ScanWindow::gap(KeyType::EcHybrid), floor.ec_hybrid);
        assert!(floor.generation >= ScanWindow::default().generation);
    }
}
