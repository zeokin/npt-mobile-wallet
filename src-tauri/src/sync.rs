// UTXO scanning: AnnouncementFlag → blockHeightsByFlags → decrypt locally.

use neptune_cash::api::export::KeyType;
use neptune_cash::prelude::triton_vm::prelude::BFieldElement;
use neptune_cash::state::wallet::address::announcement_flag::AnnouncementFlag;
use neptune_cash::state::wallet::address::{ReceivingAddress, SpendingKey};
use neptune_cash::state::wallet::wallet_entropy::WalletEntropy;
use serde::{Deserialize, Serialize};

use crate::rpc::RpcClient;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredUtxo {
    pub amount: String,
    pub block_height: u64,
    pub likely_spent: bool,
    pub key_type: String,
    pub key_index: u64,
    pub utxo_hex: String,
    pub sender_randomness_hex: String,
    pub receiver_preimage_hex: String,
    pub aocl_leaf_index: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncResult {
    pub balance: String,
    pub utxo_count: usize,
    pub blocks_scanned: usize,
    pub utxos: Vec<DiscoveredUtxo>,
}

pub async fn scan_for_utxos(
    rpc: &RpcClient, entropy: &WalletEntropy,
    num_generation_keys: u64, num_symmetric_keys: u64,
) -> Result<SyncResult, String> {
    // Derive keys and flags
    let mut keys: Vec<(SpendingKey, u64, String)> = Vec::new();
    let mut flags: Vec<AnnouncementFlag> = Vec::new();

    for i in 0..num_generation_keys {
        let sk = SpendingKey::Generation(entropy.nth_generation_spending_key(i));
        flags.push(AnnouncementFlag::from(&entropy.nth_receiving_address(i, KeyType::Generation)));
        keys.push((sk, i, "generation".to_string()));
    }
    for i in 0..num_symmetric_keys {
        let sk = SpendingKey::Symmetric(entropy.nth_symmetric_key(i));
        flags.push(AnnouncementFlag::from(&entropy.nth_receiving_address(i, KeyType::Symmetric)));
        keys.push((sk, i, "symmetric".to_string()));
    }

    if flags.is_empty() {
        return Ok(SyncResult { balance: "0".into(), utxo_count: 0, blocks_scanned: 0, utxos: vec![] });
    }

    // Query block heights matching our flags
    let block_heights = rpc.block_heights_by_flags(&flags).await?;
    let mut discovered: Vec<DiscoveredUtxo> = Vec::new();

    // Scan each block's announcements
    for height in &block_heights {
        let kernel_json = match rpc.get_block_transaction_kernel(*height).await? {
            Some(k) => k,
            None => continue,
        };
        let announcements = kernel_json.get("announcements")
            .and_then(|a| a.as_array()).cloned().unwrap_or_default();

        for ann_val in &announcements {
            let msg = match parse_announcement_message(ann_val) {
                Some(m) if m.len() >= 3 => m,
                _ => continue,
            };

            let ann_receiver_id = msg[1];
            let ciphertext: Vec<BFieldElement> = msg[2..].to_vec();

            for (key, key_index, key_type) in &keys {
                if ann_receiver_id != key.receiver_identifier() { continue; }
                if let Ok((utxo, sender_randomness)) = key.decrypt(&ciphertext) {
                    discovered.push(DiscoveredUtxo {
                        amount: format!("{}", utxo.get_native_currency_amount()),
                        block_height: *height,
                        likely_spent: false,
                        key_type: key_type.clone(),
                        key_index: *key_index,
                        utxo_hex: hex::encode(bincode::serialize(&utxo).unwrap_or_default()),
                        sender_randomness_hex: hex::encode(bincode::serialize(&sender_randomness).unwrap_or_default()),
                        receiver_preimage_hex: hex::encode(bincode::serialize(&key.privacy_preimage()).unwrap_or_default()),
                        aocl_leaf_index: None,
                    });
                }
            }
        }
    }

    // Check spent status via bloom filter
    check_spent_status(rpc, &mut discovered).await;

    let unspent: Vec<&DiscoveredUtxo> = discovered.iter().filter(|u| !u.likely_spent).collect();
    let balance = if unspent.is_empty() {
        "0".to_string()
    } else {
        let amounts: Vec<&str> = unspent.iter().map(|u| u.amount.as_str()).collect();
        format!("{} UTXOs ({})", unspent.len(), amounts.join(" + "))
    };

    Ok(SyncResult {
        balance, utxo_count: discovered.len(), blocks_scanned: block_heights.len(), utxos: discovered,
    })
}

async fn check_spent_status(rpc: &RpcClient, utxos: &mut [DiscoveredUtxo]) {
    use neptune_cash::application::json_rpc::core::model::wallet::block::RpcWalletBlock;
    use neptune_cash::protocol::consensus::block::block_kernel::BlockKernel;
    use neptune_cash::prelude::twenty_first::util_types::mmr::mmr_trait::Mmr;
    use neptune_cash::util_types::mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;

    for utxo in utxos.iter_mut() {
        if utxo.block_height == 0 { continue; }
        let Ok(prev_json) = rpc.get_wallet_blocks(utxo.block_height - 1, utxo.block_height - 1).await else { continue };
        let Ok(blocks) = serde_json::from_value::<Vec<RpcWalletBlock>>(
            prev_json.get("blocks").cloned().unwrap_or(prev_json.clone())
        ) else { continue };
        let Some(prev_rpc) = blocks.into_iter().next() else { continue };
        let prev_hash = prev_rpc.hash();
        let prev_kernel: BlockKernel = prev_rpc.kernel.into();
        let Ok(gf) = prev_kernel.guesser_fee_addition_records(prev_hash) else { continue };
        let prev_aocl = prev_kernel.body.mutator_set_accumulator_after(gf).aocl.num_leafs();

        let Ok(cur_json) = rpc.get_wallet_blocks(utxo.block_height, utxo.block_height).await else { continue };
        let Ok(cur_blocks) = serde_json::from_value::<Vec<RpcWalletBlock>>(
            cur_json.get("blocks").cloned().unwrap_or(cur_json.clone())
        ) else { continue };
        let Some(cur_rpc) = cur_blocks.into_iter().next() else { continue };
        let cur_hash = cur_rpc.hash();
        let cur_kernel: BlockKernel = cur_rpc.kernel.into();
        let Ok(adds) = cur_kernel.all_addition_records(cur_hash) else { continue };

        let Ok(utxo_bytes) = hex::decode(&utxo.utxo_hex) else { continue };
        let Ok(u) = bincode::deserialize::<neptune_cash::protocol::consensus::transaction::utxo::Utxo>(&utxo_bytes) else { continue };
        let Ok(sr_bytes) = hex::decode(&utxo.sender_randomness_hex) else { continue };
        let Ok(sr) = bincode::deserialize::<neptune_cash::prelude::triton_vm::prelude::Digest>(&sr_bytes) else { continue };
        let Ok(rp_bytes) = hex::decode(&utxo.receiver_preimage_hex) else { continue };
        let Ok(rp) = bincode::deserialize::<neptune_cash::prelude::triton_vm::prelude::Digest>(&rp_bytes) else { continue };

        let item = neptune_cash::prelude::triton_vm::prelude::Tip5::hash(&u);
        let commit = neptune_cash::util_types::mutator_set::commit(item, sr, rp.hash());

        for (i, add) in adds.iter().enumerate() {
            if add.canonical_commitment == commit.canonical_commitment {
                let idx = prev_aocl + i as u64;
                utxo.aocl_leaf_index = Some(idx);
                let abs = AbsoluteIndexSet::compute(item, sr, rp, idx);
                if let Ok(j) = serde_json::to_value(&abs) {
                    if let Ok(spent) = rpc.are_bloom_indices_set(&j).await {
                        utxo.likely_spent = spent;
                    }
                }
                break;
            }
        }
    }
}

fn parse_announcement_message(val: &serde_json::Value) -> Option<Vec<BFieldElement>> {
    if let Some(hex_str) = val.as_str() {
        let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        if hex.len() >= 32 && hex.len() % 16 == 0 {
            let bfes: Vec<BFieldElement> = hex.as_bytes().chunks(16)
                .filter_map(|c| u64::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok().map(BFieldElement::new))
                .collect();
            if !bfes.is_empty() { return Some(bfes); }
        }
    }
    if let Some(arr) = val.as_array() {
        let bfes: Vec<BFieldElement> = arr.iter().filter_map(|v| v.as_u64().map(BFieldElement::new)).collect();
        if !bfes.is_empty() { return Some(bfes); }
    }
    if let Some(msg) = val.get("message").or_else(|| val.get("0")) {
        return parse_announcement_message(msg);
    }
    None
}
