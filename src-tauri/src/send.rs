//! Send pipeline: select inputs, build, prove, submit.
//!
//! Modelled on the desktop wallet (v4.4.0) and adapted for a phone:
//! 1. Select inputs from a fresh (incremental) scan: largest first, at most
//!    [`MAX_INPUTS`].
//! 2. Read the tip header and decide whether old inputs must be lustrated —
//!    before anything about the inputs is revealed to the node.
//! 3. Fetch membership proofs and the tip header for the *same* block.
//! 4. Build outputs (recipient + change to symmetric key 0), lustration
//!    announcements, `TransactionDetails`; validate locally.
//! 5. Prove a `ProofCollection` for the tip's consensus rule set.
//! 6. Submit. If the node answers `NotConfirmable` (too many blocks arrived
//!    while proving, or an input was spent meanwhile), rebuild on the new tip
//!    with the same inputs and prove again, up to [`MAX_SEND_ATTEMPTS`] times.

use std::sync::Arc;
use std::time::Duration;

use neptune_consensus::block::block_header::BlockPow;
use neptune_consensus::consensus_rule_set::ConsensusRuleSet;
use neptune_consensus::transaction::announcement::Announcement;
use neptune_consensus::transaction::transparent_input::TransparentInput;
use neptune_consensus::transaction::Transaction;
use neptune_consensus::type_scripts::native_currency_amount::NativeCurrencyAmount;
use neptune_mempool::transaction_kernel_id::Txid;
use neptune_mutator_set::mutator_set_accumulator::MutatorSetAccumulator;
use neptune_mutator_set::removal_record::absolute_index_set::AbsoluteIndexSet;
use neptune_primitives::block_height::BlockHeight;
use neptune_primitives::network::Network;
use neptune_primitives::tasm_lib::prelude::Tip5;
use neptune_primitives::timestamp::Timestamp;
use neptune_primitives::twenty_first::util_types::mmr::mmr_trait::Mmr;
use neptune_rpc_api::api::rpc::RestoreMembershipProofError;
use neptune_rpc_api::api::rpc::RpcApi;
use neptune_rpc_api::api::rpc::RpcError;
use neptune_rpc_api::api::rpc::SubmitTransactionError;
use neptune_rpc_api::model::block::header::RpcBlockHeader;
use neptune_rpc_api::model::block::transaction_kernel::RpcAdditionRecord;
use neptune_rpc_api::model::json::JsonError;
use neptune_rpc_api::model::wallet::mutator_set::RpcMsMembershipSnapshot;
use neptune_rpc_api::model::wallet::transaction::RpcTransaction;
use neptune_wallet::address::ReceivingAddress;
use neptune_wallet::address::SpendingKey;
use neptune_wallet::transaction_details::TransactionDetails;
use neptune_wallet::transaction_output::TxOutput;
use neptune_wallet::transaction_output::TxOutputList;
use neptune_wallet::unlocked_utxo::UnlockedUtxo;
use neptune_wallet::wallet_entropy::WalletEntropy;
use num_traits::CheckedAdd;
use num_traits::CheckedSub;
use num_traits::Zero;
use serde::Serialize;

use crate::keys;
use crate::keys::KeyCache;
use crate::rpc::RpcClient;
use crate::sync::DiscoveredUtxo;
use crate::transaction;

/// Proving attempts before giving up when the node keeps answering
/// "not confirmable" (the chain moved on while proving).
pub(crate) const MAX_SEND_ATTEMPTS: usize = 3;

/// Inputs per transaction. Each input adds lock-script and removal-record
/// proving work; more than this does not fit a phone.
pub(crate) const MAX_INPUTS: usize = 10;

/// Attempts at getting membership proofs and a tip header from the same block.
const SNAPSHOT_ATTEMPTS: usize = 5;

/// Subtracted from the device clock for the transaction timestamp, so that a
/// clock running a little fast does not get the finished transaction refused
/// as future-dated (nodes tolerate one minute). Nodes keep transactions for
/// hours, so a slightly earlier timestamp costs nothing.
const TIMESTAMP_SAFETY_MARGIN_MS: u64 = 2 * 60 * 1000;

/// Progress report emitted to the UI as event `send_state`.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SendProgress {
    /// "preparing" | "proving" | "broadcasting" | "rebuilding" | "done"
    pub(crate) stage: &'static str,
    pub(crate) attempt: usize,
    pub(crate) max_attempts: usize,
    /// Sub-proof being produced ("RemovalRecordsIntegrity", …).
    pub(crate) proof: Option<String>,
    pub(crate) proof_index: usize,
    pub(crate) proof_total: usize,
}

pub(crate) type ProgressFn = Arc<dyn Fn(SendProgress) + Send + Sync>;

/// One output of a sent transaction, for display and explorer links.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct OutputInfo {
    /// Canonical commitment (hex) — identifies the output on explorers.
    pub(crate) commitment: String,
    pub(crate) amount: String,
    pub(crate) is_change: bool,
    pub(crate) address: Option<String>,
}

/// Result of a successful send.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SendOutcome {
    pub(crate) txid: String,
    pub(crate) fee: String,
    pub(crate) amount: String,
    pub(crate) outputs: Vec<OutputInfo>,
    /// `RpcAdditionRecord` JSON per output (used with `utxoindex_wasMined`).
    pub(crate) addition_record_hexes: Vec<String>,
    /// Inputs spent (AOCL leaf indices) and their absolute index sets (JSON).
    pub(crate) input_aocl_indices: Vec<u64>,
    pub(crate) input_index_sets: Vec<String>,
    pub(crate) tip_height: u64,
    pub(crate) attempts: usize,
}

/// What the user asked for.
pub(crate) struct SendRequest {
    pub(crate) recipient: String,
    pub(crate) amount: String,
    pub(crate) fee: String,
    pub(crate) accept_lustrations: bool,
}

/// Parse and sanity-check the amounts. `coins_from_str` accepts negative
/// numbers; a negative or zero amount would only be refused after minutes of
/// proving, so check up front.
pub(crate) fn parse_amounts(
    amount: &str,
    fee: &str,
) -> Result<(NativeCurrencyAmount, NativeCurrencyAmount), String> {
    let amount_val = NativeCurrencyAmount::coins_from_str(amount.trim())
        .map_err(|e| format!("Invalid amount: {}", e))?;
    let fee_val = NativeCurrencyAmount::coins_from_str(fee.trim())
        .map_err(|e| format!("Invalid fee: {}", e))?;
    if amount_val <= NativeCurrencyAmount::zero() {
        return Err("Amount must be greater than zero".to_string());
    }
    if fee_val < NativeCurrencyAmount::zero() {
        return Err("Fee must not be negative".to_string());
    }
    Ok((amount_val, fee_val))
}

/// Choose inputs covering `total_needed`: largest first.
pub(crate) fn select_inputs(
    candidates: &[DiscoveredUtxo],
    total_needed: NativeCurrencyAmount,
    at: Timestamp,
) -> Result<Vec<DiscoveredUtxo>, String> {
    let mut spendable: Vec<DiscoveredUtxo> = candidates
        .iter()
        .filter(|u| u.is_spendable_at(at))
        .cloned()
        .collect();
    let available = spendable
        .iter()
        .fold(NativeCurrencyAmount::zero(), |acc, u| acc + u.amount());
    if available < total_needed {
        let locked = candidates
            .iter()
            .filter(|u| !u.spent && u.is_locked_at(at))
            .fold(NativeCurrencyAmount::zero(), |acc, u| acc + u.amount());
        let mut msg = format!(
            "Insufficient balance: spendable {} NPT, but this transaction needs {} NPT \
             (amount plus fee).",
            available, total_needed
        );
        if locked > NativeCurrencyAmount::zero() {
            msg.push_str(&format!(" {} NPT is still time-locked.", locked));
        }
        return Err(msg);
    }

    // Largest first minimises the number of inputs (= proving work).
    spendable.sort_by(|a, b| {
        b.amount()
            .partial_cmp(&a.amount())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.aocl_leaf_index.cmp(&b.aocl_leaf_index))
    });
    let mut selected = Vec::new();
    let mut accumulated = NativeCurrencyAmount::zero();
    for input in spendable {
        accumulated = accumulated + input.amount();
        selected.push(input);
        if accumulated >= total_needed {
            break;
        }
    }
    if selected.len() > MAX_INPUTS {
        return Err(format!(
            "This amount needs {} coins (UTXOs), but a phone can prove at most {} per \
             transaction. Please send a smaller amount, or send in several steps.",
            selected.len(),
            MAX_INPUTS
        ));
    }
    Ok(selected)
}

fn transparent_inputs(inputs: &[DiscoveredUtxo]) -> Vec<TransparentInput> {
    inputs
        .iter()
        .map(|input| TransparentInput {
            utxo: input.utxo.clone(),
            aocl_leaf_index: input.aocl_leaf_index,
            sender_randomness: input.sender_randomness,
            receiver_preimage: input.receiver_preimage,
        })
        .collect()
}

/// Lustration announcements the network requires for these inputs at this
/// tip, plus the threshold (for the UI prompt).
fn lustrations(header: &RpcBlockHeader, inputs: &[DiscoveredUtxo]) -> (Vec<Announcement>, u64) {
    let pow: BlockPow = header.pow.into();
    match pow.lustration_status() {
        Ok(status) => (
            Announcement::lustration_announcements(status, &transparent_inputs(inputs)),
            status.max_lustrating_aocl_leaf_index,
        ),
        // Header without lustration status (before hard fork beta).
        Err(_) => (vec![], 0),
    }
}

fn is_retryable(e: &RpcError) -> bool {
    matches!(
        e,
        RpcError::Server(
            JsonError::ConnectionFailed { .. }
                | JsonError::HttpError { .. }
                | JsonError::InternalError
                | JsonError::ParseError
        )
    )
}

async fn tip_header(rpc: &RpcClient) -> Result<RpcBlockHeader, String> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match rpc.tip_header().await {
            Ok(resp) => return Ok(resp.header),
            Err(e) if attempt < 3 && is_retryable(&e) => {
                tokio::time::sleep(Duration::from_millis(500 * attempt)).await
            }
            Err(e) => return Err(format!("Could not read the chain tip: {e}")),
        }
    }
}

/// Membership proofs for `index_sets` and the header of the block they are
/// synced to.
async fn membership_snapshot(
    rpc: &RpcClient,
    index_sets: &[AbsoluteIndexSet],
) -> Result<(RpcMsMembershipSnapshot, RpcBlockHeader), String> {
    for attempt in 1..=SNAPSHOT_ATTEMPTS {
        let snapshot = match rpc.restore_membership_proof(index_sets.to_vec()).await {
            Ok(resp) => resp.snapshot,
            Err(RpcError::RestoreMembershipProof(RestoreMembershipProofError::TipMoved)) => {
                continue
            }
            Err(e) if is_retryable(&e) && attempt < SNAPSHOT_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
                continue;
            }
            Err(e) => return Err(format!("Could not get membership proofs: {e}")),
        };
        if snapshot.membership_proofs.len() != index_sets.len() {
            return Err(format!(
                "The node returned {} membership proofs for {} inputs",
                snapshot.membership_proofs.len(),
                index_sets.len()
            ));
        }
        let header = tip_header(rpc).await?;
        if u64::from(header.height) == snapshot.synced_height.value() {
            return Ok((snapshot, header));
        }
        debug_log!(
            "[SEND] Tip moved between membership proofs ({}) and header ({}); retrying",
            snapshot.synced_height.value(),
            u64::from(header.height)
        );
    }
    Err(
        "New blocks keep arriving; could not get a consistent view of the chain. \
         Please try again."
            .to_string(),
    )
}

/// Human-readable reason for a refused submission.
fn submit_error_message(e: &RpcError) -> String {
    match e {
        RpcError::SubmitTransaction(reason) => match reason {
            SubmitTransactionError::FutureDated => {
                "The node refused the transaction because its time is in the future. Make \
                 sure the device's date and time are set automatically, then try again."
                    .to_string()
            }
            SubmitTransactionError::TooOld => {
                "The node refused the transaction because its time is too old. Make sure the \
                 device's date and time are set automatically, then try again."
                    .to_string()
            }
            SubmitTransactionError::FeeNegative => "The fee must not be negative.".to_string(),
            SubmitTransactionError::MissingLustration => {
                "The network requires older coins in this transaction to be revealed \
                 (lustration). Please send again and confirm that step."
                    .to_string()
            }
            SubmitTransactionError::LustrationMakesCounterNegative => {
                "The network cannot accept these older coins at the moment (lustration \
                 limit reached). Please try again later."
                    .to_string()
            }
            SubmitTransactionError::TooBig => {
                "The transaction has more inputs or outputs than the network allows.".to_string()
            }
            SubmitTransactionError::InvalidTransaction => {
                "The node rejected the transaction as invalid.".to_string()
            }
            other => format!("The node refused the transaction: {other}"),
        },
        other => format!("Could not submit the transaction: {other}"),
    }
}

/// A proven transaction and what is needed to report it.
struct Built {
    transaction: Transaction,
    details: TransactionDetails,
    change: Option<TxOutput>,
    tip_height: u64,
}

#[allow(clippy::too_many_arguments)]
async fn build_once(
    rpc: &RpcClient,
    entropy: &WalletEntropy,
    key_cache: &KeyCache,
    network: Network,
    inputs: &[DiscoveredUtxo],
    recipient: &ReceivingAddress,
    amount: NativeCurrencyAmount,
    fee: NativeCurrencyAmount,
    accept_lustrations: bool,
    attempt: usize,
    progress: &ProgressFn,
) -> Result<Built, String> {
    let index_sets: Vec<AbsoluteIndexSet> = inputs.iter().map(|u| u.absolute_index_set()).collect();
    let (snapshot, header) = membership_snapshot(rpc, &index_sets).await?;
    let tip_height = u64::from(header.height);
    let tip_block_height = BlockHeight::from(tip_height);

    transaction::ensure_no_imminent_hardfork(network, tip_block_height)?;
    let consensus_rule_set = ConsensusRuleSet::infer_from(network, tip_block_height);
    if !transaction::can_prove_for(consensus_rule_set) {
        return Err(format!(
            "The node is at block {tip_height}, which is before a network upgrade this wallet \
             requires. Please use an up-to-date node."
        ));
    }

    let (lustration_announcements, threshold) = lustrations(&header, inputs);
    if !lustration_announcements.is_empty() && !accept_lustrations {
        return Err(format!("LUSTRATION_REQUIRED:{threshold}"));
    }

    let tip_msa: MutatorSetAccumulator = snapshot.synced_mutator_set.into();
    let mut unlocked_utxos = Vec::with_capacity(inputs.len());
    for (idx, (input, proof_data)) in inputs
        .iter()
        .zip(snapshot.membership_proofs.into_iter())
        .enumerate()
    {
        let membership_proof = proof_data
            .extract_ms_membership_proof(
                input.aocl_leaf_index,
                input.sender_randomness,
                input.receiver_preimage,
            )
            .ok_or(format!("Membership proof for input {} is unusable", idx))?;
        if !tip_msa.verify(Tip5::hash(&input.utxo), &membership_proof) {
            return Err(format!(
                "Coin {} could not be proven unspent (AOCL index {}, chain has {} outputs). \
                 It may have been spent meanwhile — sync and try again.",
                idx,
                input.aocl_leaf_index,
                tip_msa.aocl.num_leafs()
            ));
        }
        let spending_key = keys::spending_key_for(
            key_cache,
            entropy,
            &keys::key_type_label(input.key_type),
            input.key_index,
        )?;
        unlocked_utxos.push(UnlockedUtxo::unlock(
            input.utxo.clone(),
            spending_key.lock_script_and_witness(),
            membership_proof,
        ));
    }

    // Outputs: recipient + change (symmetric key 0, like the desktop wallet).
    let total_in = inputs
        .iter()
        .fold(NativeCurrencyAmount::zero(), |acc, u| acc + u.amount());
    let total_needed = amount.checked_add(&fee).ok_or("Amount + fee overflow")?;
    let recipient_output = TxOutput::onchain_native_currency(
        amount,
        entropy.generate_sender_randomness(tip_block_height, recipient.privacy_digest()),
        recipient.clone(),
        false,
    );
    let mut tx_outputs = TxOutputList::from(vec![recipient_output]);
    let mut change = None;
    if let Some(change_amount) = total_in.checked_sub(&total_needed) {
        if change_amount > NativeCurrencyAmount::zero() {
            let change_key = SpendingKey::Symmetric(entropy.nth_symmetric_key(0));
            let change_address = change_key.to_address();
            let change_output = TxOutput::onchain_native_currency(
                change_amount,
                entropy
                    .generate_sender_randomness(tip_block_height, change_address.privacy_digest()),
                change_address,
                true,
            );
            change = Some(change_output.clone());
            tx_outputs.push(change_output);
        }
    } else {
        return Err("Selected coins do not cover amount plus fee".to_string());
    }

    let now_ms = Timestamp::now().to_millis();
    let timestamp = Timestamp::millis(now_ms.saturating_sub(TIMESTAMP_SAFETY_MARGIN_MS));
    let mut details = TransactionDetails::new_without_coinbase(
        unlocked_utxos,
        tx_outputs,
        fee,
        timestamp,
        tip_msa,
        network,
    );
    if !lustration_announcements.is_empty() {
        details = details.with_announcements(lustration_announcements);
    }

    // Full local validation before minutes of proving: lock scripts,
    // removal records, amounts, type scripts.
    let witness = details.primitive_witness();
    let validation = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(witness.validate())
    })
    .await
    .map_err(|e| format!("Validation task error: {e}"))?;
    if let Err(e) = validation {
        return Err(format!("Transaction validation failed: {e:?}"));
    }

    let progress_for_proofs = progress.clone();
    let transaction =
        transaction::build_transaction(&details, consensus_rule_set, move |name, index, total| {
            progress_for_proofs(SendProgress {
                stage: if attempt == 1 {
                    "proving"
                } else {
                    "rebuilding"
                },
                attempt,
                max_attempts: MAX_SEND_ATTEMPTS,
                proof: Some(name.to_string()),
                proof_index: index,
                proof_total: total,
            })
        })
        .await
        .map_err(|e| format!("{e}"))?;

    Ok(Built {
        transaction,
        details,
        change,
        tip_height,
    })
}

/// Lustration requirement for `inputs` at the current tip, without revealing
/// anything about the inputs to the node. Returns the threshold if required.
pub(crate) async fn lustration_required(
    rpc: &RpcClient,
    inputs: &[DiscoveredUtxo],
) -> Result<Option<u64>, String> {
    let header = tip_header(rpc).await?;
    let (announcements, threshold) = lustrations(&header, inputs);
    Ok((!announcements.is_empty()).then_some(threshold))
}

/// Build, prove and submit a transaction paying `amount` to `recipient`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send(
    rpc: &RpcClient,
    entropy: &WalletEntropy,
    key_cache: &KeyCache,
    network: Network,
    candidates: &[DiscoveredUtxo],
    request: &SendRequest,
    progress: ProgressFn,
) -> Result<SendOutcome, String> {
    let (amount, fee) = parse_amounts(&request.amount, &request.fee)?;
    let recipient = ReceivingAddress::from_bech32m(request.recipient.trim(), network)
        .map_err(|e| format!("Invalid address: {}", e))?;
    let total_needed = amount.checked_add(&fee).ok_or("Amount + fee overflow")?;

    progress(SendProgress {
        stage: "preparing",
        attempt: 1,
        max_attempts: MAX_SEND_ATTEMPTS,
        proof: None,
        proof_index: 0,
        proof_total: 0,
    });

    let now_ms = Timestamp::now().to_millis();
    let at = Timestamp::millis(now_ms.saturating_sub(TIMESTAMP_SAFETY_MARGIN_MS));
    // Inputs are chosen once and pinned for every attempt, so the lustration
    // decision the user confirmed stays valid.
    let inputs = select_inputs(candidates, total_needed, at)?;

    // Ask about lustration before revealing the inputs to the node.
    if !request.accept_lustrations {
        if let Some(threshold) = lustration_required(rpc, &inputs).await? {
            return Err(format!("LUSTRATION_REQUIRED:{threshold}"));
        }
    }

    let mut attempt = 1;
    let built = loop {
        let built = build_once(
            rpc,
            entropy,
            key_cache,
            network,
            &inputs,
            &recipient,
            amount,
            fee,
            request.accept_lustrations,
            attempt,
            &progress,
        )
        .await?;

        progress(SendProgress {
            stage: "broadcasting",
            attempt,
            max_attempts: MAX_SEND_ATTEMPTS,
            proof: None,
            proof_index: 0,
            proof_total: 0,
        });
        let to_rpc = |tx: &Transaction| -> Result<RpcTransaction, String> {
            tx.clone()
                .try_into()
                .map_err(|e: String| format!("Convert to RPC transaction: {}", e))
        };
        match rpc.submit_transaction(to_rpc(&built.transaction)?).await {
            Ok(resp) if resp.success => break built,
            Ok(_) => {
                return Err("The node did not accept the transaction.".to_string());
            }
            Err(RpcError::SubmitTransaction(SubmitTransactionError::NotConfirmable)) => {
                debug_log!("[SEND] Node answered NotConfirmable on attempt {attempt}");
                if attempt >= MAX_SEND_ATTEMPTS {
                    return Err(format!(
                        "The node could not accept this transaction in {MAX_SEND_ATTEMPTS} \
                         attempts: each time, new blocks arrived while it was being proven, \
                         or one of its coins was spent meanwhile. Please sync and try again."
                    ));
                }
                attempt += 1;
            }
            Err(e) if is_retryable(&e) => {
                // The upload failed on the way; re-submitting the same
                // transaction is harmless (nodes accept duplicates).
                tokio::time::sleep(Duration::from_secs(2)).await;
                match rpc.submit_transaction(to_rpc(&built.transaction)?).await {
                    Ok(resp) if resp.success => break built,
                    Ok(_) => return Err("The node did not accept the transaction.".to_string()),
                    Err(e) => return Err(submit_error_message(&e)),
                }
            }
            Err(e) => return Err(submit_error_message(&e)),
        }
    };

    progress(SendProgress {
        stage: "done",
        attempt,
        max_attempts: MAX_SEND_ATTEMPTS,
        proof: None,
        proof_index: 0,
        proof_total: 0,
    });

    Ok(outcome(
        built, &inputs, &recipient, network, amount, fee, attempt,
    ))
}

fn outcome(
    built: Built,
    inputs: &[DiscoveredUtxo],
    recipient: &ReceivingAddress,
    network: Network,
    amount: NativeCurrencyAmount,
    fee: NativeCurrencyAmount,
    attempts: usize,
) -> SendOutcome {
    let change_commitment = built
        .change
        .as_ref()
        .map(|c| c.addition_record().canonical_commitment);
    let recipient_digest = recipient.privacy_digest();
    let recipient_text = recipient.to_bech32m(network).ok();
    let outputs = built
        .details
        .tx_outputs
        .iter()
        .map(|txo| {
            let commitment = txo.addition_record().canonical_commitment;
            let is_change = Some(commitment) == change_commitment;
            OutputInfo {
                commitment: commitment.to_hex(),
                amount: txo.native_currency_amount().display_lossless(),
                is_change,
                address: (!is_change && txo.receiver_digest() == recipient_digest)
                    .then(|| recipient_text.clone())
                    .flatten(),
            }
        })
        .collect();
    let kernel = built.details.transaction_kernel();
    let addition_record_hexes = kernel
        .outputs
        .iter()
        .map(|ar| serde_json::to_string(&RpcAdditionRecord::from(*ar)).unwrap_or_default())
        .collect();
    SendOutcome {
        txid: built.transaction.txid().to_string(),
        fee: fee.display_lossless(),
        amount: amount.display_lossless(),
        outputs,
        addition_record_hexes,
        input_aocl_indices: inputs.iter().map(|u| u.aocl_leaf_index).collect(),
        input_index_sets: inputs
            .iter()
            .map(|u| serde_json::to_string(&u.absolute_index_set()).unwrap_or_default())
            .collect(),
        tip_height: built.tip_height,
        attempts,
    }
}

#[cfg(test)]
mod tests {
    use neptune_consensus::transaction::utxo::Utxo;
    use neptune_primitives::tasm_lib::prelude::Digest;
    use neptune_wallet::address::KeyType;

    use super::*;

    fn coin(aocl: u64, coins: u32, spent: bool) -> DiscoveredUtxo {
        DiscoveredUtxo {
            utxo: Utxo::new_native_currency(Digest::default(), NativeCurrencyAmount::coins(coins)),
            sender_randomness: Digest::default(),
            receiver_preimage: Digest::default(),
            aocl_leaf_index: aocl,
            block_height: aocl,
            block_time: None,
            key_type: KeyType::Generation,
            key_index: 0,
            spent,
            spent_in_block: None,
        }
    }

    #[test]
    fn amounts_must_be_positive_and_fee_non_negative() {
        assert!(parse_amounts("1.5", "0.5").is_ok());
        assert!(parse_amounts("1", "0").is_ok());
        assert!(parse_amounts("0", "0.5").is_err());
        assert!(parse_amounts("-1", "0.5").is_err());
        assert!(parse_amounts("1", "-0.1").is_err());
        assert!(parse_amounts("abc", "0.5").is_err());
    }

    #[test]
    fn selection_takes_largest_unspent_coins_first() {
        let coins = vec![
            coin(1, 1, false),
            coin(2, 5, false),
            coin(3, 100, true),
            coin(4, 3, false),
        ];
        let picked =
            select_inputs(&coins, NativeCurrencyAmount::coins(7), Timestamp::now()).unwrap();
        assert_eq!(
            vec![2, 4],
            picked.iter().map(|u| u.aocl_leaf_index).collect::<Vec<_>>()
        );
    }

    #[test]
    fn insufficient_balance_is_reported_before_the_input_limit() {
        let coins: Vec<_> = (0..20).map(|i| coin(i, 1, false)).collect();
        let err =
            select_inputs(&coins, NativeCurrencyAmount::coins(50), Timestamp::now()).unwrap_err();
        assert!(err.starts_with("Insufficient balance"), "{err}");
        let err =
            select_inputs(&coins, NativeCurrencyAmount::coins(15), Timestamp::now()).unwrap_err();
        assert!(err.contains("at most 10"), "{err}");
    }
}
