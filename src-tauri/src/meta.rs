//! Per-wallet metadata kept on the device: how many receiving addresses of
//! each type have been handed out ("New address"), which of them the user
//! removed from the list, and at which of them funds were ever found.
//!
//! Every handed-out address is scanned — also a removed one, so a payment to
//! it still shows up (and brings it back to the list). Indices at which funds
//! were found count as handed out (like the desktop wallet).
//!
//! Survives a rescan (unlike the scan cache); removed when the wallet is
//! replaced or deleted.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use neptune_wallet::address::KeyType;
use serde::Deserialize;
use serde::Serialize;

use crate::sync::ScanWindow;

const META_FILE: &str = "wallet_meta.json";
/// v2 added `hidden` and `funded`; v1 files load with both empty.
const META_VERSION: u32 = 2;

/// Upper bound of handed-out addresses per type.
pub(crate) const MAX_ISSUED_ADDRESSES: u64 = 1_000;

/// A set of address indices per receiving key type.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct IndexSets {
    #[serde(default)]
    generation: BTreeSet<u64>,
    #[serde(default)]
    ec_hybrid: BTreeSet<u64>,
    #[serde(default)]
    viewing_address: BTreeSet<u64>,
}

impl IndexSets {
    fn get(&self, key_type: KeyType) -> Option<&BTreeSet<u64>> {
        match key_type {
            KeyType::Generation => Some(&self.generation),
            KeyType::EcHybrid => Some(&self.ec_hybrid),
            KeyType::ViewingAddress => Some(&self.viewing_address),
            _ => None,
        }
    }

    fn get_mut(&mut self, key_type: KeyType) -> Option<&mut BTreeSet<u64>> {
        match key_type {
            KeyType::Generation => Some(&mut self.generation),
            KeyType::EcHybrid => Some(&mut self.ec_hybrid),
            KeyType::ViewingAddress => Some(&mut self.viewing_address),
            _ => None,
        }
    }

    fn contains(&self, key_type: KeyType, index: u64) -> bool {
        self.get(key_type).is_some_and(|set| set.contains(&index))
    }

    /// Keep only indices for which `keep` holds.
    fn retain(&mut self, keep: impl Fn(KeyType, u64) -> bool) {
        for key_type in [
            KeyType::Generation,
            KeyType::EcHybrid,
            KeyType::ViewingAddress,
        ] {
            if let Some(set) = self.get_mut(key_type) {
                set.retain(|&index| keep(key_type, index));
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WalletMeta {
    version: u32,
    /// Next index to hand out, per type. Index 0 is the main address, so the
    /// minimum is 1. Never decreases: removing an address only hides it.
    generation: u64,
    ec_hybrid: u64,
    viewing_address: u64,
    /// Addresses the user removed from the list (never index 0).
    #[serde(default)]
    hidden: IndexSets,
    /// Addresses at which funds were found; they cannot be removed.
    #[serde(default)]
    funded: IndexSets,
}

impl Default for WalletMeta {
    fn default() -> Self {
        Self {
            version: META_VERSION,
            generation: 1,
            ec_hybrid: 1,
            viewing_address: 1,
            hidden: IndexSets::default(),
            funded: IndexSets::default(),
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
        let mut meta = Self {
            version: META_VERSION,
            generation: meta.generation.clamp(1, MAX_ISSUED_ADDRESSES),
            ec_hybrid: meta.ec_hybrid.clamp(1, MAX_ISSUED_ADDRESSES),
            viewing_address: meta.viewing_address.clamp(1, MAX_ISSUED_ADDRESSES),
            hidden: meta.hidden,
            funded: meta.funded,
        };
        // Only handed-out indices can be hidden or funded; index 0 is never
        // hidden, and a funded address is never hidden.
        let next = meta.clone();
        meta.funded.retain(|kt, index| index < next.next_index(kt));
        meta.hidden.retain(|kt, index| {
            index >= 1 && index < next.next_index(kt) && !next.funded.contains(kt, index)
        });
        meta
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

    /// Handed-out addresses of `key_type` that are listed (not removed),
    /// newest first. Always ends with index 0.
    pub(crate) fn listed(&self, key_type: KeyType) -> Vec<u64> {
        (0..self.next_index(key_type))
            .rev()
            .filter(|&index| !self.hidden.contains(key_type, index))
            .collect()
    }

    /// Index of the newest listed address of `key_type` (the one shown for
    /// receiving).
    pub(crate) fn current_index(&self, key_type: KeyType) -> u64 {
        self.listed(key_type).first().copied().unwrap_or(0)
    }

    /// Whether funds were ever found at this address.
    pub(crate) fn is_funded(&self, key_type: KeyType, index: u64) -> bool {
        self.funded.contains(key_type, index)
    }

    /// Whether the user may remove this address from the list: an address
    /// they added (not the main address #0) that never received funds.
    pub(crate) fn can_remove(&self, key_type: KeyType, index: u64) -> bool {
        index >= 1
            && index < self.next_index(key_type)
            && !self.hidden.contains(key_type, index)
            && !self.funded.contains(key_type, index)
    }

    /// Remove an address from the list. It keeps being scanned.
    pub(crate) fn remove(&mut self, key_type: KeyType, index: u64) -> Result<(), String> {
        if index == 0 {
            return Err("The main address (#0) cannot be removed".to_string());
        }
        if index >= self.next_index(key_type) || self.hidden.contains(key_type, index) {
            return Err(format!("Address #{index} is not in the list"));
        }
        if self.funded.contains(key_type, index) {
            return Err(format!(
                "Address #{index} has received coins, so it stays in the list"
            ));
        }
        match self.hidden.get_mut(key_type) {
            Some(set) => {
                set.insert(index);
                Ok(())
            }
            None => Err("This key type has no shareable addresses".to_string()),
        }
    }

    /// Hand out the next address index of `key_type`. Removed addresses are
    /// never handed out again.
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

    /// Record the indices at which funds were found: they count as handed
    /// out, cannot be removed, and come back to the list if they were
    /// removed. Returns whether anything changed.
    pub(crate) fn absorb_found(&mut self, found: impl IntoIterator<Item = (KeyType, u64)>) -> bool {
        let before = self.clone();
        for (key_type, index) in found {
            if index >= MAX_ISSUED_ADDRESSES {
                continue;
            }
            if index + 1 > self.next_index(key_type) {
                self.set_next_index(key_type, index + 1);
            }
            if let Some(set) = self.funded.get_mut(key_type) {
                set.insert(index);
            }
            if let Some(set) = self.hidden.get_mut(key_type) {
                set.remove(&index);
            }
        }
        before != *self
    }

    /// Smallest scan window covering every handed-out address (removed ones
    /// included) plus the usual look-ahead gap.
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

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("npt-meta-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn issuing_addresses_advances_and_persists() {
        let dir = temp_dir("issue");
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
        assert!(meta.absorb_found([(KeyType::EcHybrid, 2)])); // newly funded
        assert!(!meta.absorb_found([(KeyType::EcHybrid, 2)])); // nothing new
        let floor = meta.scan_floor();
        assert_eq!(7 + ScanWindow::gap(KeyType::EcHybrid), floor.ec_hybrid);
        assert!(floor.generation >= ScanWindow::default().generation);
    }

    #[test]
    fn removed_addresses_leave_the_list_but_stay_scanned() {
        let mut meta = WalletMeta::default();
        for _ in 0..3 {
            meta.issue(KeyType::ViewingAddress).unwrap(); // #1, #2, #3
        }
        let floor_before = meta.scan_floor();

        // The main address can never be removed.
        assert!(!meta.can_remove(KeyType::ViewingAddress, 0));
        assert!(meta.remove(KeyType::ViewingAddress, 0).is_err());
        // Not handed out yet.
        assert!(meta.remove(KeyType::ViewingAddress, 4).is_err());

        // Removing the newest makes the previous one current.
        assert!(meta.can_remove(KeyType::ViewingAddress, 3));
        meta.remove(KeyType::ViewingAddress, 3).unwrap();
        assert_eq!(2, meta.current_index(KeyType::ViewingAddress));
        assert_eq!(vec![2, 1, 0], meta.listed(KeyType::ViewingAddress));
        assert!(meta.remove(KeyType::ViewingAddress, 3).is_err()); // already removed

        // Removing one in the middle keeps the newest current.
        meta.remove(KeyType::ViewingAddress, 1).unwrap();
        assert_eq!(vec![2, 0], meta.listed(KeyType::ViewingAddress));

        // Removed addresses are still scanned, and never handed out again.
        assert_eq!(floor_before, meta.scan_floor());
        assert_eq!(4, meta.issue(KeyType::ViewingAddress).unwrap());
        assert_eq!(4, meta.current_index(KeyType::ViewingAddress));

        // Other types are unaffected.
        assert_eq!(vec![0], meta.listed(KeyType::EcHybrid));
    }

    #[test]
    fn funded_addresses_cannot_be_removed_and_come_back_when_paid() {
        let mut meta = WalletMeta::default();
        meta.issue(KeyType::EcHybrid).unwrap(); // #1
        meta.issue(KeyType::EcHybrid).unwrap(); // #2
        meta.absorb_found([(KeyType::EcHybrid, 1)]);
        assert!(meta.is_funded(KeyType::EcHybrid, 1));
        assert!(!meta.can_remove(KeyType::EcHybrid, 1));
        assert!(meta.remove(KeyType::EcHybrid, 1).is_err());

        // A removed address that later receives a payment is listed again.
        meta.remove(KeyType::EcHybrid, 2).unwrap();
        assert_eq!(1, meta.current_index(KeyType::EcHybrid));
        assert!(meta.absorb_found([(KeyType::EcHybrid, 2)]));
        assert_eq!(2, meta.current_index(KeyType::EcHybrid));
        assert!(!meta.can_remove(KeyType::EcHybrid, 2));
    }

    #[test]
    fn version_1_files_load_and_inconsistent_sets_are_cleaned() {
        let dir = temp_dir("v1");
        std::fs::write(
            dir.join(META_FILE),
            br#"{"version":1,"generation":1,"ec_hybrid":4,"viewing_address":2}"#,
        )
        .unwrap();
        let meta = WalletMeta::load(&dir);
        assert_eq!(4, meta.next_index(KeyType::EcHybrid));
        assert_eq!(vec![3, 2, 1, 0], meta.listed(KeyType::EcHybrid));

        // Hidden #0, hidden-but-funded #2 and out-of-range indices are dropped.
        std::fs::write(
            dir.join(META_FILE),
            br#"{"version":2,"generation":1,"ec_hybrid":4,"viewing_address":1,
                "hidden":{"ec_hybrid":[0,2,3,9]},"funded":{"ec_hybrid":[2,7]}}"#,
        )
        .unwrap();
        let meta = WalletMeta::load(&dir);
        assert_eq!(vec![2, 1, 0], meta.listed(KeyType::EcHybrid));
        assert!(meta.is_funded(KeyType::EcHybrid, 2));
        assert!(!meta.is_funded(KeyType::EcHybrid, 7));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
