#![cfg(feature = "enable")]
use alloy_primitives::{Address, B256};
use alloy_sol_types::{sol, SolType};
use raiko_lib::input::{GuestInput, Transition};
use serde::{Deserialize, Serialize};
use sp1_sdk::{HashableKey, ProverClient, SP1Stdin};
use std::path::PathBuf;

/// The public values encoded as a tuple that can be easily deserialized inside Solidity.
type ProtocolInstanceTuple = sol! {
    tuple(string, uint64, address, Transition, address, address, bytes32)
};

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaikoProofFixture {
    pub chain_id: u64,
    pub verifier_address: Address,
    pub transition: Transition,
    pub sgx_instance: Address, // only used for SGX
    pub prover: Address,
    meta_hash: B256,
    // "VERIFY_PROOF", _chainId, _verifierContract, _tran, _newInstance, _prover, _metaHash
    vkey: String,
    public_values: String,
    proof: String,
}

fn main() {
    // Setup the logger.
    sp1_sdk::utils::setup_logger();

    // Setup the prover client.
    let client = ProverClient::new();

    // Setup the program.
    let (pk, vk) = client.setup(sp1_driver::ELF);

    // Setup the inputs.;
    let mut stdin = SP1Stdin::new();
    stdin.write(&GuestInput::default());

    // Generate the proof.
    let proof = client
        .prove_groth16(&pk, stdin)
        .expect("failed to generate proof");

    // Deserialize the public values.
    let bytes = proof.public_values.as_slice();
    let (
        verify_proof, 
        chain_id,
        verifier_address,
        transition, 
        sgx_instance, 
        prover, 
        meta_hash
    ) = ProtocolInstanceTuple::abi_decode(bytes, false).unwrap();

    // Create the testing fixture so we can test things end-ot-end.
    let fixture = RaikoProofFixture {
        chain_id,
        verifier_address,
        transition, 
        sgx_instance, 
        prover, 
        meta_hash,
        vkey: vk.bytes32().to_string(),
        public_values: proof.public_values.bytes().to_string(),
        proof: proof.bytes().to_string(),
    };


    // Save the fixture to a file.
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../contracts/src/fixtures");
    std::fs::create_dir_all(&fixture_path).expect("failed to create fixture path");
    std::fs::write(
        fixture_path.join("fixture.json"),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .expect("failed to write fixture");

    let contracts_src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../contracts/src");
    sp1_sdk::artifacts::export_solidity_groth16_verifier(contracts_src_dir)
        .expect("failed to export verifier");
}
