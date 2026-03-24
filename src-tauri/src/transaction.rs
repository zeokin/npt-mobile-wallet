//! Transaction building module — constructs and broadcasts transactions locally.
//!
//! Flow:
//! 1. Select input UTXOs from local store
//! 2. Create output UTXOs (recipient + change)
//! 3. Fetch membership proofs + mutator set accumulator from supporter
//! 4. Build TransactionDetails → PrimitiveWitness
//! 5. Generate ProofCollection (STARK proofs via Triton VM) — CPU intensive
//! 6. Submit via wallet_submitTransaction RPC

use anyhow::Result;
use itertools::Itertools;
use neptune_cash::api::export::Tip5;
use neptune_cash::prelude::tasm_lib;
use neptune_cash::prelude::triton_vm::proof::Proof;
use neptune_cash::prelude::triton_vm::prove;
use neptune_cash::prelude::triton_vm::stark::Stark;
use neptune_cash::prelude::triton_vm::vm::{NonDeterminism, PublicInput};
use neptune_cash::protocol::consensus::transaction::primitive_witness::PrimitiveWitness;
use neptune_cash::protocol::consensus::transaction::transaction_kernel::TransactionKernelField;
use neptune_cash::protocol::consensus::transaction::validity::collect_lock_scripts::CollectLockScriptsWitness;
use neptune_cash::protocol::consensus::transaction::validity::collect_type_scripts::CollectTypeScriptsWitness;
use neptune_cash::protocol::consensus::transaction::validity::kernel_to_outputs::KernelToOutputsWitness;
use neptune_cash::protocol::consensus::transaction::validity::proof_collection::ProofCollection;
use neptune_cash::protocol::consensus::transaction::validity::removal_records_integrity::RemovalRecordsIntegrityWitness;
use neptune_cash::protocol::consensus::transaction::Transaction;
use neptune_cash::protocol::consensus::transaction::TransactionProof;
use neptune_cash::protocol::proof_abstractions::mast_hash::MastHash;
use neptune_cash::protocol::proof_abstractions::SecretWitness;
use neptune_cash::state::transaction::transaction_details::TransactionDetails;
use serde::{Deserialize, Serialize};
use tasm_lib::triton_vm::prelude::Program;
use tasm_lib::triton_vm::proof::Claim;

/// Result of a send operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendResult {
    pub txid: String,
    pub status: String,
}

/// Build a Transaction from TransactionDetails.
///
/// This generates a ProofCollection (multiple STARK proofs) which is
/// CPU-intensive and may take several minutes.
pub async fn build_transaction(
    transaction_details: &TransactionDetails,
) -> Result<Transaction> {
    let primitive_witness =
        PrimitiveWitness::from_transaction_details(transaction_details);

    let kernel = primitive_witness.kernel.clone();

    // Generate ProofCollection in a blocking task to avoid blocking the async runtime
    let proof = tokio::task::spawn_blocking(move || {
        produce_proof_collection(&primitive_witness)
    })
    .await??;

    Ok(Transaction {
        kernel,
        proof: TransactionProof::ProofCollection(proof),
    })
}

/// Generate a ProofCollection from a PrimitiveWitness.
///
/// This produces 4 + N_lock + N_type individual STARK proofs:
/// - removal_records_integrity: input UTXOs are in mutator set
/// - collect_lock_scripts: lock scripts collected correctly
/// - lock_scripts_halt: each lock script halts (1 per input)
/// - kernel_to_outputs: kernel matches outputs
/// - collect_type_scripts: type scripts collected correctly
/// - type_scripts_halt: each type script halts (1 per type)
fn produce_proof_collection(
    primitive_witness: &PrimitiveWitness,
) -> Result<ProofCollection> {
    let removal_records_integrity_witness =
        RemovalRecordsIntegrityWitness::from(primitive_witness);
    let collect_lock_scripts_witness = CollectLockScriptsWitness::from(primitive_witness);
    let kernel_to_outputs_witness = KernelToOutputsWitness::from(primitive_witness);
    let collect_type_scripts_witness = CollectTypeScriptsWitness::from(primitive_witness);

    let txk_mast_hash = primitive_witness.kernel.mast_hash();
    let txk_mast_hash_as_input = PublicInput::new(txk_mast_hash.reversed().values().to_vec());
    let salted_inputs_hash = Tip5::hash(&primitive_witness.input_utxos);
    let salted_outputs_hash = Tip5::hash(&primitive_witness.output_utxos);

    // Prove each component
    let removal_records_integrity = produce_proof(
        removal_records_integrity_witness.program(),
        removal_records_integrity_witness.claim(),
        removal_records_integrity_witness.nondeterminism(),
    )?
    .into();

    let collect_lock_scripts = produce_proof(
        collect_lock_scripts_witness.program(),
        collect_lock_scripts_witness.claim(),
        collect_lock_scripts_witness.nondeterminism(),
    )?
    .into();

    let kernel_to_outputs = produce_proof(
        kernel_to_outputs_witness.program(),
        kernel_to_outputs_witness.claim(),
        kernel_to_outputs_witness.nondeterminism(),
    )?
    .into();

    let collect_type_scripts = produce_proof(
        collect_type_scripts_witness.program(),
        collect_type_scripts_witness.claim(),
        collect_type_scripts_witness.nondeterminism(),
    )?
    .into();

    // Prove lock scripts (1 per input UTXO)
    let mut lock_scripts_halt = vec![];
    for lsaw in &primitive_witness.lock_scripts_and_witnesses {
        let claim = Claim::new(lsaw.program.hash())
            .with_input(txk_mast_hash_as_input.clone().individual_tokens);
        let proof = produce_proof(
            lsaw.program.clone(),
            claim,
            lsaw.nondeterminism(),
        )?
        .into();
        lock_scripts_halt.push(proof);
    }

    // Prove type scripts (1 per type)
    let mut type_scripts_halt = vec![];
    for tsaw in &primitive_witness.type_scripts_and_witnesses {
        let input: Vec<_> = [txk_mast_hash, salted_inputs_hash, salted_outputs_hash]
            .into_iter()
            .flat_map(|d| d.reversed().values())
            .collect();
        let claim = Claim::new(tsaw.program.hash()).with_input(input);
        let proof = produce_proof(tsaw.program.clone(), claim, tsaw.nondeterminism())?.into();
        type_scripts_halt.push(proof);
    }

    // Collect hashes
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

/// Run the Triton VM prover for a single claim.
fn produce_proof(
    program: Program,
    claim: Claim,
    non_determinism: NonDeterminism,
) -> Result<Proof> {
    let stark = Stark::default();
    let proof = prove(stark, &claim, program, non_determinism)?;
    Ok(proof)
}
