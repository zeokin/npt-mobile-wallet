//! Transaction building module — constructs and broadcasts transactions locally.
//!
//! Uses two-step proving approach (adapted from XNT wallet):
//! 1. VM::trace_execution() — fast, just runs the program
//! 2. Check padded_height complexity against limit
//! 3. If within limit → stark.prove() — the expensive STARK proof
//! 4. If over limit → fast fail with clear error message
//!
//! This prevents mobile devices from attempting proofs that would
//! take too long or crash with OOM.

use anyhow::anyhow;
use anyhow::Result;
use itertools::Itertools;
use neptune_consensus::consensus_rule_set::ConsensusRuleSet;
use neptune_consensus::consensus_rule_set::TritonProofVersion;
use neptune_consensus::proof_abstractions::tasm::legacy_stark_verify::claim_uses_legacy_proof_system;
use neptune_consensus::proof_abstractions::tasm::legacy_stark_verify::LegacyProverPipeline;
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

/// Maximum log2 padded height for proof generation.
const MAX_LOG2_PADDED_HEIGHT: u8 = 17;

/// Number of blocks before a consensus hardfork during which sending is
/// refused. A transaction built under the old rules would be rejected once the
/// new rules activate, and proving takes some time.
const HARDFORK_SEND_FREEZE_BLOCKS: usize = 3;

/// Refuse to build a transaction if a hardfork activates within the next
/// [`HARDFORK_SEND_FREEZE_BLOCKS`] blocks.
pub(crate) fn ensure_no_imminent_hardfork(
    network: Network,
    tip: BlockHeight,
) -> Result<(), String> {
    let rules_now = ConsensusRuleSet::infer_from(network, tip);
    let rules_soon = ConsensusRuleSet::infer_from(network, tip + HARDFORK_SEND_FREEZE_BLOCKS);
    if rules_now != rules_soon {
        return Err(format!(
            "A consensus hardfork activates within the next {HARDFORK_SEND_FREEZE_BLOCKS} \
             blocks (tip: {tip}). Sending is paused until it has activated."
        ));
    }
    Ok(())
}

/// The version stamped on every Triton VM claim under the given rule set.
/// Verifiers reconstruct the claim with this version, so the proof must be
/// generated for it.
fn proof_version(consensus_rule_set: ConsensusRuleSet) -> u32 {
    match consensus_rule_set.triton_proof_version() {
        TritonProofVersion::V0 => 0,
        TritonProofVersion::V1 => 1,
        TritonProofVersion::V5 => 5,
        TritonProofVersion::V8 => 8,
    }
}

/// Build a Transaction from TransactionDetails.
///
/// Uses two-step approach: trace first (fast), then prove (slow).
/// Fails fast if any proof exceeds the mobile complexity limit.
pub(crate) async fn build_transaction(
    transaction_details: &TransactionDetails,
    consensus_rule_set: ConsensusRuleSet,
) -> Result<Transaction> {
    let primitive_witness = transaction_details.primitive_witness();
    let kernel = primitive_witness.kernel.clone();

    let proof = tokio::task::spawn_blocking(move || {
        produce_proof_collection(&primitive_witness, consensus_rule_set, |p, c, n, name| {
            prove_with_limit(p, c, n, Some(MAX_LOG2_PADDED_HEIGHT), name)
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

    // Prove each component with complexity check
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
///
/// Step 1: VM::trace_execution() — fast, just runs the program
/// Step 2: Check padded_height against limit — instant
/// Step 3: stark.prove() — expensive STARK proof generation
///
/// If complexity exceeds limit, returns error BEFORE the expensive step.
///
/// The claim's version selects the proof system.
fn prove_with_limit(
    program: Program,
    claim: Claim,
    non_determinism: NonDeterminism,
    max_log2_padded_height: Option<u8>,
    proof_name: &str,
) -> Result<Proof> {
    if claim_uses_legacy_proof_system(&claim) {
        // The legacy pipeline panics on a failing execution, so run the
        // program (both VM generations share the ISA) to surface errors first.
        let public_output = VM::run(
            program.clone(),
            claim.input.clone().into(),
            non_determinism.clone(),
        )
        .map_err(|e| anyhow!("{} VM execution failed: {}", proof_name, e))?;
        if public_output != claim.output {
            return Err(anyhow!(
                "{} VM output does not match claim output",
                proof_name
            ));
        }

        let pipeline = LegacyProverPipeline::trace(&program, &claim, non_determinism);
        check_padded_height(
            pipeline.log2_padded_height(),
            max_log2_padded_height,
            proof_name,
        )?;
        return Ok(pipeline.prove());
    }

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
    check_padded_height(
        aet.padded_height().ilog2() as u8,
        max_log2_padded_height,
        proof_name,
    )?;

    // Step 3: STARK prove (expensive)
    let stark = Stark::default();
    let proof = stark
        .prove(&claim, &aet)
        .map_err(|e| anyhow!("{} STARK proving failed: {}", proof_name, e))?;

    Ok(proof)
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
                "Transaction too complex — proof generation would take longer than \
                 the block time (~10 minutes), which would invalidate the transaction. \
                 Please send a smaller amount to use fewer UTXOs. \
                 (Proof '{}': 2^{} rows exceeds limit 2^{})",
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
    use neptune_consensus::consensus_rule_set::BLOCK_HEIGHT_HARDFORK_DELTA_MAIN_NET;
    use neptune_consensus::type_scripts::native_currency_amount::NativeCurrencyAmount;
    use neptune_mutator_set::ms_membership_proof::MsMembershipProof;
    use neptune_mutator_set::mutator_set_accumulator::MutatorSetAccumulator;
    use neptune_primitives::timestamp::Timestamp;
    use neptune_wallet::address::KeyType;
    use neptune_wallet::address::SpendingKey;
    use neptune_wallet::transaction_output::TxOutput;
    use neptune_wallet::unlocked_utxo::UnlockedUtxo;
    use num_traits::CheckedSub;

    use super::*;
    use crate::keys::wallet_entropy_from_phrase;

    const BOTH_SIDES_OF_HARDFORK_DELTA: [ConsensusRuleSet; 2] = [
        ConsensusRuleSet::HardforkGamma,
        ConsensusRuleSet::HardforkDelta,
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
        let amount = utxo.get_native_currency_amount().checked_sub(&fee).unwrap();
        let recipient = entropy.nth_receiving_address(1, KeyType::Generation);
        let output = TxOutput::onchain_native_currency(amount, sender_randomness, recipient, false);
        TransactionDetails::new_without_coinbase(
            vec![UnlockedUtxo::unlock(
                utxo,
                key.lock_script_and_witness(),
                membership_proof,
            )],
            vec![output],
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
    fn claims_match_upstream_verifier_on_both_sides_of_hardfork_delta() {
        let primitive_witness = devnet_premine_transaction_details().primitive_witness();
        let expected = ProofCollection::produce_mock(&primitive_witness, true);

        let mut claims_per_rule_set = vec![];
        for rules in BOTH_SIDES_OF_HARDFORK_DELTA {
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
            claims_per_rule_set.push(claims);
        }

        assert_ne!(
            claims_per_rule_set[0], claims_per_rule_set[1],
            "the hardfork changes every claim"
        );
    }

    #[test]
    fn sending_is_frozen_for_three_blocks_before_hardfork_delta() {
        let activation = u64::from(BLOCK_HEIGHT_HARDFORK_DELTA_MAIN_NET);
        let guard = |tip: u64| ensure_no_imminent_hardfork(Network::Main, BlockHeight::from(tip));
        for tip in [activation - 4, activation, activation + 1] {
            assert!(guard(tip).is_ok(), "tip {tip} must allow sending");
        }
        for tip in [activation - 3, activation - 2, activation - 1] {
            assert!(guard(tip).is_err(), "tip {tip} must refuse sending");
        }
    }

    #[tokio::test]
    async fn transaction_verifies_only_under_the_rule_set_it_was_built_for() {
        let primitive_witness = devnet_premine_transaction_details().primitive_witness();
        for built_under in BOTH_SIDES_OF_HARDFORK_DELTA {
            let pw = primitive_witness.clone();
            let proof_collection = tokio::task::spawn_blocking(move || {
                produce_proof_collection(&pw, built_under, |p, c, n, name| {
                    prove_with_limit(p, c, n, None, name)
                })
            })
            .await
            .unwrap()
            .unwrap();
            let transaction = Transaction {
                kernel: primitive_witness.kernel.clone(),
                proof: TransactionProof::ProofCollection(proof_collection),
            };
            for verified_under in BOTH_SIDES_OF_HARDFORK_DELTA {
                assert_eq!(
                    built_under == verified_under,
                    transaction.is_valid(Network::Main, verified_under).await,
                    "built under {built_under}, verified under {verified_under}"
                );
            }
        }
    }
}
