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
use neptune_cash::api::export::Tip5;
use neptune_cash::api::export::TransactionDetails;
use neptune_cash::api::export::TransactionProof;
use neptune_cash::prelude::tasm_lib;
use neptune_cash::prelude::triton_vm::proof::Proof;
use neptune_cash::prelude::triton_vm::stark::Stark;
use neptune_cash::prelude::triton_vm::vm::NonDeterminism;
use neptune_cash::prelude::triton_vm::vm::PublicInput;
use neptune_cash::prelude::triton_vm::vm::VM;
use neptune_cash::protocol::consensus::transaction::primitive_witness::PrimitiveWitness;
use neptune_cash::protocol::consensus::transaction::transaction_kernel::TransactionKernelField;
use neptune_cash::protocol::consensus::transaction::validity::collect_lock_scripts::CollectLockScriptsWitness;
use neptune_cash::protocol::consensus::transaction::validity::collect_type_scripts::CollectTypeScriptsWitness;
use neptune_cash::protocol::consensus::transaction::validity::kernel_to_outputs::KernelToOutputsWitness;
use neptune_cash::protocol::consensus::transaction::validity::proof_collection::ProofCollection;
use neptune_cash::protocol::consensus::transaction::validity::removal_records_integrity::RemovalRecordsIntegrityWitness;
use neptune_cash::protocol::consensus::transaction::Transaction;
use neptune_cash::protocol::proof_abstractions::mast_hash::MastHash;
use neptune_cash::protocol::proof_abstractions::SecretWitness;
use tasm_lib::triton_vm::prelude::Program;
use tasm_lib::triton_vm::proof::Claim;

/// Maximum log2 padded height for proof generation.
/// Must complete within ~9 minutes (before next block invalidates proofs).
/// Testing results: 2^23 takes ~3 min on desktop.
/// 2^24 would take ~6-8 min. 2^25 would risk exceeding block time.
const MAX_LOG2_PADDED_HEIGHT: u8 = 24;

/// Build a Transaction from TransactionDetails.
///
/// Uses two-step approach: trace first (fast), then prove (slow).
/// Fails fast if any proof exceeds the mobile complexity limit.
pub(crate) async fn build_transaction(
    transaction_details: &TransactionDetails,
) -> Result<Transaction> {
    let primitive_witness = PrimitiveWitness::from_transaction_details(transaction_details);
    let kernel = primitive_witness.kernel.clone();

    let proof = tokio::task::spawn_blocking(move || {
        produce_proof_collection(&primitive_witness, Some(MAX_LOG2_PADDED_HEIGHT))
    })
    .await??;

    Ok(Transaction {
        kernel,
        proof: TransactionProof::ProofCollection(proof),
    })
}

/// Generate a ProofCollection with optional complexity limit.
///
/// If `max_log2_padded_height` is Some, each proof is checked after
/// VM execution but BEFORE the expensive STARK proving step.
fn produce_proof_collection(
    primitive_witness: &PrimitiveWitness,
    max_log2_padded_height: Option<u8>,
) -> Result<ProofCollection> {
    let removal_records_integrity_witness = RemovalRecordsIntegrityWitness::from(primitive_witness);
    let collect_lock_scripts_witness = CollectLockScriptsWitness::from(primitive_witness);
    let kernel_to_outputs_witness = KernelToOutputsWitness::from(primitive_witness);
    let collect_type_scripts_witness = CollectTypeScriptsWitness::from(primitive_witness);

    let txk_mast_hash = primitive_witness.kernel.mast_hash();
    let txk_mast_hash_as_input = PublicInput::new(txk_mast_hash.reversed().values().to_vec());
    let salted_inputs_hash = Tip5::hash(&primitive_witness.input_utxos);
    let salted_outputs_hash = Tip5::hash(&primitive_witness.output_utxos);

    // Prove each component with complexity check
    let removal_records_integrity = prove_with_limit(
        removal_records_integrity_witness.program(),
        removal_records_integrity_witness.claim(),
        removal_records_integrity_witness.nondeterminism(),
        max_log2_padded_height,
        "RemovalRecordsIntegrity",
    )?
    .into();

    let collect_lock_scripts = prove_with_limit(
        collect_lock_scripts_witness.program(),
        collect_lock_scripts_witness.claim(),
        collect_lock_scripts_witness.nondeterminism(),
        max_log2_padded_height,
        "CollectLockScripts",
    )?
    .into();

    let kernel_to_outputs = prove_with_limit(
        kernel_to_outputs_witness.program(),
        kernel_to_outputs_witness.claim(),
        kernel_to_outputs_witness.nondeterminism(),
        max_log2_padded_height,
        "KernelToOutputs",
    )?
    .into();

    let collect_type_scripts = prove_with_limit(
        collect_type_scripts_witness.program(),
        collect_type_scripts_witness.claim(),
        collect_type_scripts_witness.nondeterminism(),
        max_log2_padded_height,
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
            .with_input(txk_mast_hash_as_input.clone().individual_tokens);
        let proof = prove_with_limit(
            lsaw.program.clone(),
            claim,
            lsaw.nondeterminism(),
            max_log2_padded_height,
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
        let claim = Claim::new(tsaw.program.hash()).with_input(input);
        let proof = prove_with_limit(
            tsaw.program.clone(),
            claim,
            tsaw.nondeterminism(),
            max_log2_padded_height,
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
fn prove_with_limit(
    program: Program,
    claim: Claim,
    non_determinism: NonDeterminism,
    max_log2_padded_height: Option<u8>,
    proof_name: &str,
) -> Result<Proof> {
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
    debug_log!(
        "[PROOF] {} padded_height: 2^{} = {} rows",
        proof_name,
        log2_padded_height,
        aet.padded_height()
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

    // Step 3: STARK prove (expensive)
    let stark = Stark::default();
    let proof = stark
        .prove(&claim, &aet)
        .map_err(|e| anyhow!("{} STARK proving failed: {}", proof_name, e))?;

    Ok(proof)
}
