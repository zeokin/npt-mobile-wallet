//! Transaction building module — constructs and broadcasts transactions locally.
//!
//! The wallet builds transactions entirely on-device:
//! 1. Select input UTXOs from local store
//! 2. Fetch membership proofs from supporter
//! 3. Build PrimitiveWitness locally
//! 4. Generate ProofCollection (STARK proofs via Triton VM)
//! 5. Submit via wallet_submitTransaction RPC
//!
//! The supporter never sees the spending keys or the raw witness.

// TODO: This module is a placeholder for Phase 3 implementation.
// The actual transaction building requires:
// - PrimitiveWitness construction from local UTXOs
// - ProofCollection generation (CPU-intensive, may take minutes)
// - Transaction kernel assembly
// - RPC submission
//
// Key types needed (all public in neptune-cash):
// - PrimitiveWitness { input_utxos, input_membership_proofs, lock_scripts_and_witnesses,
//                      type_scripts_and_witnesses, output_utxos, output_sender_randomnesses,
//                      output_receiver_digests, mutator_set_accumulator, kernel }
// - TransactionBuilder::new().transaction_kernel(kernel).transaction_proof(proof).build()
// - ProofCollection generation from PrimitiveWitness
//
// Implementation will follow neptune-wallet-app patterns adapted for nimble client.

use serde::{Deserialize, Serialize};

/// Status of a transaction being built.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TxBuildStatus {
    SelectingInputs,
    FetchingProofs,
    BuildingWitness,
    GeneratingProofs,
    Submitting,
    Complete { txid: String },
    Failed { error: String },
}

/// Result of a send operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendResult {
    pub txid: String,
    pub status: String,
}
