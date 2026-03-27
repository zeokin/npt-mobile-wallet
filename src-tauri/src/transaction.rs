// ProofCollection generation with mobile complexity limit.
// Two-step: trace execution → check complexity → prove.

use anyhow::{anyhow, Result};
use itertools::Itertools;
use neptune_cash::api::export::{Tip5, TransactionDetails, TransactionProof};
use neptune_cash::prelude::tasm_lib;
use neptune_cash::prelude::triton_vm::proof::Proof;
use neptune_cash::prelude::triton_vm::stark::Stark;
use neptune_cash::prelude::triton_vm::vm::{NonDeterminism, PublicInput, VM};
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

// Must complete within ~9 min (before next block invalidates proofs)
const MAX_LOG2_PADDED_HEIGHT: u8 = 24;

pub async fn build_transaction(td: &TransactionDetails) -> Result<Transaction> {
    let pw = PrimitiveWitness::from_transaction_details(td);
    let kernel = pw.kernel.clone();
    let proof = tokio::task::spawn_blocking(move || produce_proof_collection(&pw, Some(MAX_LOG2_PADDED_HEIGHT))).await??;
    Ok(Transaction { kernel, proof: TransactionProof::ProofCollection(proof) })
}

fn produce_proof_collection(pw: &PrimitiveWitness, max_height: Option<u8>) -> Result<ProofCollection> {
    let rri = RemovalRecordsIntegrityWitness::from(pw);
    let cls = CollectLockScriptsWitness::from(pw);
    let kto = KernelToOutputsWitness::from(pw);
    let cts = CollectTypeScriptsWitness::from(pw);

    let txk_hash = pw.kernel.mast_hash();
    let txk_input = PublicInput::new(txk_hash.reversed().values().to_vec());
    let si_hash = Tip5::hash(&pw.input_utxos);
    let so_hash = Tip5::hash(&pw.output_utxos);

    let removal_records_integrity = prove_with_limit(rri.program(), rri.claim(), rri.nondeterminism(), max_height, "RemovalRecordsIntegrity")?.into();
    let collect_lock_scripts = prove_with_limit(cls.program(), cls.claim(), cls.nondeterminism(), max_height, "CollectLockScripts")?.into();
    let kernel_to_outputs = prove_with_limit(kto.program(), kto.claim(), kto.nondeterminism(), max_height, "KernelToOutputs")?.into();
    let collect_type_scripts = prove_with_limit(cts.program(), cts.claim(), cts.nondeterminism(), max_height, "CollectTypeScripts")?.into();

    let mut lock_scripts_halt = vec![];
    for (i, lsaw) in pw.lock_scripts_and_witnesses.iter().enumerate() {
        let claim = Claim::new(lsaw.program.hash()).with_input(txk_input.clone().individual_tokens);
        lock_scripts_halt.push(prove_with_limit(lsaw.program.clone(), claim, lsaw.nondeterminism(), max_height, &format!("LockScript[{}]", i))?.into());
    }

    let mut type_scripts_halt = vec![];
    for (i, tsaw) in pw.type_scripts_and_witnesses.iter().enumerate() {
        let input: Vec<_> = [txk_hash, si_hash, so_hash].into_iter().flat_map(|d| d.reversed().values()).collect();
        let claim = Claim::new(tsaw.program.hash()).with_input(input);
        type_scripts_halt.push(prove_with_limit(tsaw.program.clone(), claim, tsaw.nondeterminism(), max_height, &format!("TypeScript[{}]", i))?.into());
    }

    Ok(ProofCollection {
        removal_records_integrity, collect_lock_scripts, lock_scripts_halt,
        kernel_to_outputs, collect_type_scripts, type_scripts_halt,
        lock_script_hashes: pw.lock_scripts_and_witnesses.iter().map(|l| l.program.hash()).collect_vec(),
        type_script_hashes: pw.type_scripts_and_witnesses.iter().map(|t| t.program.hash()).collect_vec(),
        kernel_mast_hash: txk_hash, salted_inputs_hash: si_hash, salted_outputs_hash: so_hash,
        merge_bit_mast_path: pw.kernel.mast_path(TransactionKernelField::MergeBit),
    })
}

fn prove_with_limit(program: Program, claim: Claim, nd: NonDeterminism, max: Option<u8>, name: &str) -> Result<Proof> {
    let (aet, output) = VM::trace_execution(program.clone(), claim.input.clone().into(), nd.clone())
        .map_err(|e| anyhow!("{} VM failed: {}", name, e))?;
    if output != claim.output { return Err(anyhow!("{} output mismatch", name)); }

    let log2 = aet.padded_height().ilog2() as u8;
    if let Some(m) = max {
        if log2 > m {
            return Err(anyhow!("Transaction too complex — proof '{}' needs 2^{} rows (limit 2^{}). Send a smaller amount.", name, log2, m));
        }
    }

    Ok(Stark::default().prove(&claim, &aet).map_err(|e| anyhow!("{} prove failed: {}", name, e))?)
}
