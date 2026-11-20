//! Transaction proving module — turns `TransactionDetails` into a
//! `ProofCollection`-backed transaction.
//!
//! Two-step proving per sub-proof:
//! 1. `VM::trace_execution()` — fast, just runs the program.
//! 2. Check the padded height (≈ memory/time cost) against the device limit.
//! 3. If within the limit → `Stark::prove()`, the expensive STARK proof.
//! 4. If over the limit → fail fast with a clear message instead of letting
//!    the phone run out of memory.
//!
//! Proofs are made for the consensus rule set of the block the transaction
//! builds on, with Triton VM 9 (claim version 8, verified by every node since
//! hard fork delta at main net block 55,000).

use anyhow::anyhow;
use anyhow::Result;
use itertools::Itertools;
use neptune_consensus::consensus_rule_set::ConsensusRuleSet;
use neptune_consensus::consensus_rule_set::TritonProofVersion;
use neptune_consensus::proof_abstractions::SecretWitness;
use neptune_consensus::tasm_lib;
use neptune_consensus::tasm_lib::prelude::Tip5;
use neptune_consensus::transaction::primitive_witness::PrimitiveWitness;
use neptune_consensus::transaction::transaction_kernel::TransactionKernelField;
use neptune_consensus::transaction::validity::collect_lock_scripts::CollectLockScriptsWitness;
use neptune_consensus::transaction::validity::collect_type_scripts::CollectTypeScriptsWitness;
use neptune_consensus::transaction::validity::kernel_to_outputs::KernelToOutputsWitness;
use neptune_consensus::transaction::validity::proof_collection::ProofCollection;
use neptune_consensus::transaction::validity::removal_records_integrity::RemovalRecordsIntegrityWitness;
use neptune_consensus::transaction::Transaction;
use neptune_consensus::transaction::TransactionProof;
use neptune_consensus::triton_vm::proof::Proof;
use neptune_consensus::triton_vm::proof::CURRENT_VERSION;
use neptune_consensus::triton_vm::stark::Stark;
use neptune_consensus::triton_vm::vm::NonDeterminism;
use neptune_consensus::triton_vm::vm::PublicInput;
use neptune_consensus::triton_vm::vm::VM;
use neptune_primitives::block_height::BlockHeight;
use neptune_primitives::mast_hash::MastHash;
use neptune_primitives::network::Network;
use neptune_wallet::transaction_details::TransactionDetails;
use tasm_lib::triton_vm::prelude::Program;
use tasm_lib::triton_vm::proof::Claim;

/// Maximum log2 padded height for proof generation on the device.
///
/// Memory use grows with the padded height; 2^17 rows stays within the RAM of
/// current phones (2^21 needs tens of GB). Ordinary transactions with a few
/// inputs stay below it.
pub(crate) const MAX_LOG2_PADDED_HEIGHT: u8 = 17;

/// Number of blocks before a consensus fork during which sending is refused.
/// A transaction built under the old rules could be invalid once the new rules
/// activate, and proving on a phone takes a while.
const HARDFORK_SEND_FREEZE_BLOCKS: usize = 3;

/// Refuse to build a transaction if a fork activates within the next
/// [`HARDFORK_SEND_FREEZE_BLOCKS`] blocks.
pub(crate) fn ensure_no_imminent_hardfork(
    network: Network,
    tip: BlockHeight,
) -> Result<(), String> {
    let rules_now = ConsensusRuleSet::infer_from(network, tip);
    let rules_soon = ConsensusRuleSet::infer_from(network, tip + HARDFORK_SEND_FREEZE_BLOCKS);
    if rules_now != rules_soon {
        return Err(format!(
            "A network upgrade ({rules_soon}) activates within the next \
             {HARDFORK_SEND_FREEZE_BLOCKS} blocks (tip: {tip}). Sending is paused until \
             it has activated — please try again in about half an hour."
        ));
    }
    Ok(())
}

/// The version stamped on every Triton VM claim under the given rule set.
/// Verifiers reconstruct the claim with this version, so the proof must be
/// generated for it.
pub(crate) fn proof_version(consensus_rule_set: ConsensusRuleSet) -> u32 {
    match consensus_rule_set.triton_proof_version() {
        TritonProofVersion::V0 => 0,
        TritonProofVersion::V1 => 1,
        TritonProofVersion::V5 => 5,
        TritonProofVersion::V8 => 8,
    }
}

/// Whether the linked Triton VM produces proofs that nodes accept under this
/// rule set. Only the current proof version can be produced; older rule sets
/// (before hard fork delta) used provers that are no longer linked.
pub(crate) fn can_prove_for(consensus_rule_set: ConsensusRuleSet) -> bool {
    proof_version(consensus_rule_set) == CURRENT_VERSION
}

/// Build a Transaction from TransactionDetails.
///
/// Uses the two-step approach: trace first (fast), then prove (slow). Fails
/// fast if any proof exceeds the device limit. `on_proof` is told the name of
/// each sub-proof just before it is proven (for progress reporting).
pub(crate) async fn build_transaction(
    transaction_details: &TransactionDetails,
    consensus_rule_set: ConsensusRuleSet,
    on_proof: impl Fn(&str, usize, usize) + Send + Sync + 'static,
) -> Result<Transaction> {
    let primitive_witness = transaction_details.primitive_witness();
    let kernel = primitive_witness.kernel.clone();

    let proof = tokio::task::spawn_blocking(move || {
        let total = 4
            + primitive_witness.lock_scripts_and_witnesses.len()
            + primitive_witness.type_scripts_and_witnesses.len();
        let done = std::sync::atomic::AtomicUsize::new(0);
        produce_proof_collection(&primitive_witness, consensus_rule_set, |p, c, n, name| {
            let i = done.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            on_proof(name, i + 1, total);
            prove_with_limit(p, c, n, Some(MAX_LOG2_PADDED_HEIGHT), name).map(|(proof, _)| proof)
        })
    })
    .await??;

    Ok(Transaction {
        kernel,
        proof: TransactionProof::ProofCollection(proof),
    })
}

/// Generate a ProofCollection under the given consensus rules.
///
/// Every claim is stamped with the rule set's proof version, and `prove` is
/// called once per (program, claim, nondeterminism, name) to produce its proof.
fn produce_proof_collection(
    primitive_witness: &PrimitiveWitness,
    consensus_rule_set: ConsensusRuleSet,
    prove: impl Fn(Program, Claim, NonDeterminism, &str) -> Result<Proof>,
) -> Result<ProofCollection> {
    let proof_version = proof_version(consensus_rule_set);
    let removal_records_integrity_witness = RemovalRecordsIntegrityWitness::from(primitive_witness);
    let collect_lock_scripts_witness = CollectLockScriptsWitness::from(primitive_witness);
    let kernel_to_outputs_witness = KernelToOutputsWitness::from(primitive_witness);
    let collect_type_scripts_witness = CollectTypeScriptsWitness::from(primitive_witness);

    let txk_mast_hash = primitive_witness.kernel.mast_hash();
    let txk_mast_hash_as_input = PublicInput::new(txk_mast_hash.reversed().values().to_vec());
    let salted_inputs_hash = Tip5::hash(&primitive_witness.input_utxos);
    let salted_outputs_hash = Tip5::hash(&primitive_witness.output_utxos);

    let removal_records_integrity = prove(
        removal_records_integrity_witness.program(),
        removal_records_integrity_witness
            .claim()
            .about_version(proof_version),
        removal_records_integrity_witness.nondeterminism(),
        "RemovalRecordsIntegrity",
    )?
    .into();

    let collect_lock_scripts = prove(
        collect_lock_scripts_witness.program(),
        collect_lock_scripts_witness
            .claim()
            .about_version(proof_version),
        collect_lock_scripts_witness.nondeterminism(),
        "CollectLockScripts",
    )?
    .into();

    let kernel_to_outputs = prove(
        kernel_to_outputs_witness.program(),
        kernel_to_outputs_witness
            .claim()
            .about_version(proof_version),
        kernel_to_outputs_witness.nondeterminism(),
        "KernelToOutputs",
    )?
    .into();

    let collect_type_scripts = prove(
        collect_type_scripts_witness.program(),
        collect_type_scripts_witness
            .claim()
            .about_version(proof_version),
        collect_type_scripts_witness.nondeterminism(),
        "CollectTypeScripts",
    )?
    .into();

    // Prove lock scripts (1 per input UTXO)
    let mut lock_scripts_halt = vec![];
    for (i, lsaw) in primitive_witness
        .lock_scripts_and_witnesses
        .iter()
        .enumerate()
    {
        let claim = Claim::new(lsaw.program.hash())
            .about_version(proof_version)
            .with_input(txk_mast_hash_as_input.clone().individual_tokens);
        let proof = prove(
            lsaw.program.clone(),
            claim,
            lsaw.nondeterminism(),
            &format!("LockScript[{}]", i),
        )?
        .into();
        lock_scripts_halt.push(proof);
    }

    // Prove type scripts (1 per type)
    let mut type_scripts_halt = vec![];
    for (i, tsaw) in primitive_witness
        .type_scripts_and_witnesses
        .iter()
        .enumerate()
    {
        let input: Vec<_> = [txk_mast_hash, salted_inputs_hash, salted_outputs_hash]
            .into_iter()
            .flat_map(|d| d.reversed().values())
            .collect();
        let claim = Claim::new(tsaw.program.hash())
            .about_version(proof_version)
            .with_input(input);
        let proof = prove(
            tsaw.program.clone(),
            claim,
            tsaw.nondeterminism(),
            &format!("TypeScript[{}]", i),
        )?
        .into();
        type_scripts_halt.push(proof);
    }

    let lock_script_hashes = primitive_witness
        .lock_scripts_and_witnesses
        .iter()
        .map(|lsaw| lsaw.program.hash())
        .collect_vec();
    let type_script_hashes = primitive_witness
        .type_scripts_and_witnesses
        .iter()
        .map(|tsaw| tsaw.program.hash())
        .collect_vec();

    let merge_bit_mast_path = primitive_witness
        .kernel
        .mast_path(TransactionKernelField::MergeBit);

    Ok(ProofCollection {
        removal_records_integrity,
        collect_lock_scripts,
        lock_scripts_halt,
        kernel_to_outputs,
        collect_type_scripts,
        type_scripts_halt,
        lock_script_hashes,
        type_script_hashes,
        kernel_mast_hash: txk_mast_hash,
        salted_inputs_hash,
        salted_outputs_hash,
        merge_bit_mast_path,
    })
}

/// Two-step prove: trace execution first, check complexity, then prove.
/// Returns the proof and the log2 padded height it was made at.
fn prove_with_limit(
    program: Program,
    claim: Claim,
    non_determinism: NonDeterminism,
    max_log2_padded_height: Option<u8>,
    proof_name: &str,
) -> Result<(Proof, u8)> {
    // Step 1: Trace execution (fast)
    let claim_input = claim.input.clone();
    let (aet, public_output) =
        VM::trace_execution(program.clone(), claim_input.into(), non_determinism.clone())
            .map_err(|e| anyhow!("{} VM execution failed: {}", proof_name, e))?;

    // Verify output matches claim
    if public_output != claim.output {
        return Err(anyhow!(
            "{} VM output does not match claim output",
            proof_name
        ));
    }

    // Step 2: Check complexity (instant)
    let log2_padded_height = aet.padded_height().ilog2() as u8;
    check_padded_height(log2_padded_height, max_log2_padded_height, proof_name)?;

    // Step 3: STARK prove (expensive)
    let stark = Stark::default();
    let proof = stark
        .prove(&claim, &aet)
        .map_err(|e| anyhow!("{} STARK proving failed: {}", proof_name, e))?;

    Ok((proof, log2_padded_height))
}

fn check_padded_height(
    log2_padded_height: u8,
    max_log2_padded_height: Option<u8>,
    proof_name: &str,
) -> Result<()> {
    debug_log!(
        "[PROOF] {} padded_height: 2^{} rows",
        proof_name,
        log2_padded_height
    );
    if let Some(max) = max_log2_padded_height {
        if log2_padded_height > max {
            return Err(anyhow!(
                "Transaction too large to prove on this device. Please send a smaller \
                 amount so that fewer coins are needed. \
                 (Proof '{}': 2^{} rows exceeds the limit of 2^{})",
                proof_name,
                log2_padded_height,
                max
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use neptune_consensus::block::Block;
    use neptune_consensus::consensus_rule_set::BLOCK_HEIGHT_SOFTFORK_EPSILON_MAIN_NET;
    use neptune_consensus::type_scripts::native_currency_amount::NativeCurrencyAmount;
    use neptune_mutator_set::ms_membership_proof::MsMembershipProof;
    use neptune_mutator_set::mutator_set_accumulator::MutatorSetAccumulator;
    use neptune_primitives::timestamp::Timestamp;
    use neptune_wallet::address::KeyType;
    use neptune_wallet::address::SpendingKey;
    use neptune_wallet::transaction_output::TxOutput;
    use neptune_wallet::unlocked_utxo::UnlockedUtxo;
    use num_traits::CheckedSub;
    use num_traits::Zero;

    use super::*;
    use crate::keys::wallet_entropy_from_phrase;

    const ALL_RULE_SETS: [ConsensusRuleSet; 7] = [
        ConsensusRuleSet::Reboot,
        ConsensusRuleSet::HardforkAlpha,
        ConsensusRuleSet::TvmProofVersion1,
        ConsensusRuleSet::HardforkBeta,
        ConsensusRuleSet::HardforkGamma,
        ConsensusRuleSet::HardforkDelta,
        ConsensusRuleSet::SoftforkEpsilon,
    ];

    /// The rule sets main net transactions are built under today.
    const CURRENT_RULE_SETS: [ConsensusRuleSet; 2] = [
        ConsensusRuleSet::HardforkDelta,
        ConsensusRuleSet::SoftforkEpsilon,
    ];

    /// A transaction spending the devnet premine UTXO, with its membership
    /// proof rebuilt from the genesis block's addition records.
    fn devnet_premine_transaction_details() -> TransactionDetails {
        let network = Network::Main;
        let devnet_mnemonic = [
            "margin", "quality", "divorce", "tuition", "notable", "squirrel", "park", "jar", "end",
            "beauty", "attend", "cliff", "media", "letter", "private", "decline", "absurd",
            "uniform",
        ]
        .map(str::to_string);
        let entropy = wallet_entropy_from_phrase(&devnet_mnemonic).unwrap();
        let key: SpendingKey = entropy.nth_generation_spending_key(0).into();

        let premine_utxos = Block::premine_utxos();
        let index = premine_utxos
            .iter()
            .position(|utxo| utxo.lock_script_hash() == key.to_address().lock_script_hash())
            .expect("devnet key owns a premine UTXO");
        let utxo = premine_utxos[index].clone();
        let item = Tip5::hash(&utxo);
        let sender_randomness = Block::premine_sender_randomness(network);
        let receiver_preimage = key.privacy_preimage();

        let genesis = Block::genesis(network);
        let mut msa = MutatorSetAccumulator::default();
        let mut membership_proof = None;
        for (i, addition_record) in genesis
            .kernel
            .body
            .transaction_kernel
            .outputs
            .iter()
            .enumerate()
        {
            if i == index {
                membership_proof = Some(msa.prove(item, sender_randomness, receiver_preimage));
            } else if let Some(mp) = membership_proof.as_mut() {
                MsMembershipProof::batch_update_from_addition(
                    &mut [mp],
                    &[item],
                    &msa,
                    addition_record,
                )
                .unwrap();
            }
            msa.add(addition_record);
        }
        let membership_proof = membership_proof.unwrap();
        assert!(msa.verify(item, &membership_proof));
        assert_eq!(
            genesis.mutator_set_accumulator_after().unwrap().hash(),
            msa.hash()
        );

        let fee = NativeCurrencyAmount::coins(1);
        let change_key = SpendingKey::Symmetric(entropy.nth_symmetric_key(0));
        let send = NativeCurrencyAmount::coins(5);
        let change = utxo
            .get_native_currency_amount()
            .checked_sub(&fee)
            .unwrap()
            .checked_sub(&send)
            .unwrap();
        let recipient = entropy.nth_receiving_address(1, KeyType::Generation);
        // Recipient + change: the shape of an ordinary send.
        let outputs = vec![
            TxOutput::onchain_native_currency(send, sender_randomness, recipient, false),
            TxOutput::onchain_native_currency(
                change,
                sender_randomness,
                change_key.to_address(),
                true,
            ),
        ];
        TransactionDetails::new_without_coinbase(
            vec![UnlockedUtxo::unlock(
                utxo,
                key.lock_script_and_witness(),
                membership_proof,
            )],
            outputs,
            fee,
            Timestamp::now(),
            msa,
            network,
        )
    }

    /// The claims the wallet proves, keyed by proof name, without proving.
    fn claims_proved_under(
        primitive_witness: &PrimitiveWitness,
        consensus_rule_set: ConsensusRuleSet,
    ) -> (ProofCollection, Vec<(String, Claim)>) {
        let claims = Mutex::new(vec![]);
        let proof_collection = produce_proof_collection(
            primitive_witness,
            consensus_rule_set,
            |_, claim, _, name| {
                claims.lock().unwrap().push((name.to_string(), claim));
                Ok(Proof(vec![]))
            },
        )
        .unwrap();
        (proof_collection, claims.into_inner().unwrap())
    }

    #[test]
    fn claims_match_upstream_verifier_for_every_provable_rule_set() {
        let primitive_witness = devnet_premine_transaction_details().primitive_witness();
        let expected = ProofCollection::produce_mock(&primitive_witness, true);

        for rules in ALL_RULE_SETS.into_iter().filter(|r| can_prove_for(*r)) {
            let (proof_collection, claims) = claims_proved_under(&primitive_witness, rules);
            let named = |prefix: &str| {
                claims
                    .iter()
                    .filter(|(name, _)| name.starts_with(prefix))
                    .map(|(_, claim)| claim.clone())
                    .collect_vec()
            };
            assert_eq!(
                vec![expected.removal_records_integrity_claim(rules)],
                named("RemovalRecordsIntegrity")
            );
            assert_eq!(
                vec![expected.collect_lock_scripts_claim(rules)],
                named("CollectLockScripts")
            );
            assert_eq!(
                vec![expected.kernel_to_outputs_claim(rules)],
                named("KernelToOutputs")
            );
            assert_eq!(
                vec![expected.collect_type_scripts_claim(rules)],
                named("CollectTypeScripts")
            );
            assert_eq!(expected.lock_script_claims(rules), named("LockScript["));
            assert_eq!(expected.type_script_claims(rules), named("TypeScript["));

            assert_eq!(expected.kernel_mast_hash, proof_collection.kernel_mast_hash);
            assert_eq!(
                expected.salted_inputs_hash,
                proof_collection.salted_inputs_hash
            );
            assert_eq!(
                expected.salted_outputs_hash,
                proof_collection.salted_outputs_hash
            );
            assert_eq!(
                expected.lock_script_hashes,
                proof_collection.lock_script_hashes
            );
            assert_eq!(
                expected.type_script_hashes,
                proof_collection.type_script_hashes
            );
            assert_eq!(
                expected.merge_bit_mast_path,
                proof_collection.merge_bit_mast_path
            );
        }
    }

    #[test]
    fn proves_from_hardfork_delta_on() {
        for rules in ALL_RULE_SETS {
            let expected = matches!(
                rules,
                ConsensusRuleSet::HardforkDelta | ConsensusRuleSet::SoftforkEpsilon
            );
            assert_eq!(expected, can_prove_for(rules), "{rules}");
        }
        // The rule set of today's main net tip.
        assert!(can_prove_for(ConsensusRuleSet::infer_from(
            Network::Main,
            BlockHeight::from(63_600u64)
        )));
    }

    #[test]
    fn sending_is_frozen_for_three_blocks_before_softfork_epsilon() {
        let activation = u64::from(BLOCK_HEIGHT_SOFTFORK_EPSILON_MAIN_NET);
        let guard = |tip: u64| ensure_no_imminent_hardfork(Network::Main, BlockHeight::from(tip));
        for tip in [activation - 4, activation, activation + 1] {
            assert!(guard(tip).is_ok(), "tip {tip} must allow sending");
        }
        for tip in [activation - 3, activation - 2, activation - 1] {
            assert!(guard(tip).is_err(), "tip {tip} must refuse sending");
        }
    }

    /// Transaction spending `num_inputs` UTXOs of the devnet key, built
    /// against a mutator set with `history` earlier outputs (so membership
    /// paths have a realistic length).
    fn transaction_with_inputs(num_inputs: usize, history: u64) -> TransactionDetails {
        use neptune_consensus::transaction::utxo::Utxo;
        use neptune_mutator_set::addition_record::AdditionRecord;
        use neptune_mutator_set::commit;
        use neptune_primitives::tasm_lib::prelude::Digest;
        use neptune_primitives::triton_vm::prelude::BFieldElement;

        let network = Network::Main;
        let devnet_mnemonic = [
            "margin", "quality", "divorce", "tuition", "notable", "squirrel", "park", "jar", "end",
            "beauty", "attend", "cliff", "media", "letter", "private", "decline", "absurd",
            "uniform",
        ]
        .map(str::to_string);
        let entropy = wallet_entropy_from_phrase(&devnet_mnemonic).unwrap();
        let key: SpendingKey = entropy.nth_ec_hybrid_key(0).into();

        let mut msa = MutatorSetAccumulator::default();
        for i in 0..history {
            msa.add(&AdditionRecord::new(Digest::new(
                [BFieldElement::new(i + 1); 5],
            )));
        }

        let coins_each = NativeCurrencyAmount::coins(10);
        let mut items = vec![];
        let mut proofs: Vec<MsMembershipProof> = vec![];
        let mut utxos = vec![];
        for i in 0..num_inputs {
            let utxo = Utxo::new_native_currency(key.lock_script_hash(), coins_each);
            let item = Tip5::hash(&utxo);
            let sender_randomness = Digest::new([BFieldElement::new(1_000 + i as u64); 5]);
            let receiver_preimage = key.privacy_preimage();
            let addition_record = commit(item, sender_randomness, receiver_preimage.hash());
            {
                let mut refs: Vec<&mut MsMembershipProof> = proofs.iter_mut().collect();
                MsMembershipProof::batch_update_from_addition(
                    &mut refs,
                    &items,
                    &msa,
                    &addition_record,
                )
                .unwrap();
            }
            proofs.push(msa.prove(item, sender_randomness, receiver_preimage));
            msa.add(&addition_record);
            items.push(item);
            utxos.push(utxo);
        }
        for (item, proof) in items.iter().zip(&proofs) {
            assert!(msa.verify(*item, proof));
        }

        let fee = NativeCurrencyAmount::coins(1);
        let total = utxos.iter().fold(NativeCurrencyAmount::zero(), |acc, u| {
            acc + u.get_native_currency_amount()
        });
        let send = NativeCurrencyAmount::coins(5);
        let change = total.checked_sub(&fee).unwrap().checked_sub(&send).unwrap();
        let randomness = Digest::new([BFieldElement::new(77); 5]);
        let outputs = vec![
            TxOutput::onchain_native_currency(
                send,
                randomness,
                entropy.nth_receiving_address(3, KeyType::Generation),
                false,
            ),
            TxOutput::onchain_native_currency(
                change,
                randomness,
                SpendingKey::Symmetric(entropy.nth_symmetric_key(0)).to_address(),
                true,
            ),
        ];
        let inputs: Vec<UnlockedUtxo> = utxos
            .into_iter()
            .zip(proofs)
            .map(|(utxo, proof)| UnlockedUtxo::unlock(utxo, key.lock_script_and_witness(), proof))
            .collect();
        TransactionDetails::new_without_coinbase(
            inputs,
            outputs,
            fee,
            Timestamp::now(),
            msa,
            network,
        )
    }

    /// log2 padded height of every sub-proof, measured by tracing only.
    fn traced_heights(details: &TransactionDetails) -> Vec<(String, u8)> {
        let heights = Mutex::new(vec![]);
        produce_proof_collection(
            &details.primitive_witness(),
            ConsensusRuleSet::SoftforkEpsilon,
            |program, claim, nd, name| {
                let (aet, output) = VM::trace_execution(program, claim.input.clone().into(), nd)
                    .map_err(|e| anyhow!("{name}: {e}"))?;
                assert_eq!(claim.output, output, "{name}");
                heights
                    .lock()
                    .unwrap()
                    .push((name.to_string(), aet.padded_height().ilog2() as u8));
                Ok(Proof(vec![]))
            },
        )
        .unwrap();
        heights.into_inner().unwrap()
    }

    /// How many inputs fit the device limit: traces 1..=MAX_INPUTS-input
    /// transactions (no proving) and checks that `MAX_INPUTS` is consistent
    /// with `MAX_LOG2_PADDED_HEIGHT`.
    #[test]
    fn input_limit_matches_the_device_proving_limit() {
        let mut fits = 0;
        for n in 1..=crate::send::MAX_INPUTS + 2 {
            let heights = traced_heights(&transaction_with_inputs(n, 4_096));
            let worst = heights.iter().max_by_key(|(_, h)| *h).unwrap().clone();
            println!(
                "{n} input(s): max 2^{} ({}) — {heights:?}",
                worst.1, worst.0
            );
            if worst.1 <= MAX_LOG2_PADDED_HEIGHT {
                fits = n;
            } else {
                break;
            }
        }
        println!("inputs that fit 2^{MAX_LOG2_PADDED_HEIGHT}: {fits}");
        assert!(fits >= 2, "an ordinary send must fit the device limit");
        assert!(
            crate::send::MAX_INPUTS <= fits,
            "MAX_INPUTS ({}) exceeds what fits 2^{MAX_LOG2_PADDED_HEIGHT} ({fits})",
            crate::send::MAX_INPUTS
        );
    }

    /// Proves a real transaction (slow: minutes, several GB of RAM) and checks
    /// it with the same verifier nodes use. Also asserts that an ordinary
    /// 1-input, 2-output transaction fits the device limit.
    #[tokio::test(flavor = "multi_thread")]
    async fn proven_transaction_is_valid_for_current_rules_and_fits_the_device_limit() {
        let primitive_witness = devnet_premine_transaction_details().primitive_witness();
        let heights = std::sync::Arc::new(Mutex::new(vec![]));
        let pw = primitive_witness.clone();
        let recorded = heights.clone();
        let proof_collection = tokio::task::spawn_blocking(move || {
            produce_proof_collection(&pw, ConsensusRuleSet::SoftforkEpsilon, |p, c, n, name| {
                let (proof, log2) = prove_with_limit(p, c, n, None, name)?;
                recorded.lock().unwrap().push((name.to_string(), log2));
                Ok(proof)
            })
        })
        .await
        .unwrap()
        .unwrap();

        let heights = heights.lock().unwrap().clone();
        println!("log2 padded heights: {heights:?}");
        for (name, log2) in &heights {
            assert!(
                *log2 <= MAX_LOG2_PADDED_HEIGHT,
                "{name} needs 2^{log2} rows, above the device limit"
            );
        }

        let transaction = Transaction {
            kernel: primitive_witness.kernel.clone(),
            proof: TransactionProof::ProofCollection(proof_collection),
        };
        for rules in CURRENT_RULE_SETS {
            assert!(
                transaction.is_valid(Network::Main, rules).await,
                "must be valid under {rules}"
            );
        }
        assert!(
            !transaction
                .is_valid(Network::Main, ConsensusRuleSet::HardforkGamma)
                .await,
            "claims of another proof version must not verify"
        );
    }
}
