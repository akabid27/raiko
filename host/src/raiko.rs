use alloy_primitives::FixedBytes;
use raiko_lib::builder::{BlockBuilderStrategy, TaikoStrategy};
use raiko_lib::consts::{ChainSpec, VerifierType};
use raiko_lib::input::{GuestInput, GuestOutput, TaikoProverData};
use raiko_lib::protocol_instance::ProtocolInstance;
use raiko_lib::prover::{to_proof, Proof, Prover, ProverError, ProverResult};
use raiko_lib::utils::HeaderHasher;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tracing::{error, info, trace, warn};

use crate::preflight::preflight;
use crate::{
    interfaces::{
        error::{self, HostError, HostResult},
        request::ProofRequest,
    },
    provider::BlockDataProvider,
};

pub struct Raiko {
    l1_chain_spec: ChainSpec,
    taiko_chain_spec: ChainSpec,
    request: ProofRequest,
}

impl Raiko {
    pub fn new(
        l1_chain_spec: ChainSpec,
        taiko_chain_spec: ChainSpec,
        request: ProofRequest,
    ) -> Self {
        Self {
            l1_chain_spec,
            taiko_chain_spec,
            request,
        }
    }

    pub async fn generate_input<BDP: BlockDataProvider>(
        &self,
        provider: BDP,
    ) -> HostResult<GuestInput> {
        preflight(
            provider,
            self.request.block_number,
            self.l1_chain_spec.clone(),
            self.taiko_chain_spec.clone(),
            TaikoProverData {
                graffiti: self.request.graffiti,
                prover: self.request.prover,
            },
        )
        .await
        .map_err(Into::<error::HostError>::into)
    }

    pub fn get_output(&self, input: &GuestInput) -> HostResult<GuestOutput> {
        match TaikoStrategy::build_from(input) {
            Ok((header, _mpt_node)) => {
                info!("Verifying final state using provider data ...");
                info!("Final block hash derived successfully. {}", header.hash());
                info!("Final block header derived successfully. {header:?}");
                let pi = ProtocolInstance::new(input, &header, VerifierType::None)?.instance_hash();

                // Check against the expected value of all fields for easy debugability
                let exp = &input.block_header_reference;
                check_eq(&exp.parent_hash, &header.parent_hash, "base_fee_per_gas");
                check_eq(&exp.ommers_hash, &header.ommers_hash, "ommers_hash");
                check_eq(&exp.beneficiary, &header.beneficiary, "beneficiary");
                check_eq(&exp.state_root, &header.state_root, "state_root");
                check_eq(
                    &exp.transactions_root,
                    &header.transactions_root,
                    "transactions_root",
                );
                check_eq(&exp.receipts_root, &header.receipts_root, "receipts_root");
                check_eq(
                    &exp.withdrawals_root,
                    &header.withdrawals_root,
                    "withdrawals_root",
                );
                check_eq(&exp.logs_bloom, &header.logs_bloom, "logs_bloom");
                check_eq(&exp.difficulty, &header.difficulty, "difficulty");
                check_eq(&exp.number, &header.number, "number");
                check_eq(&exp.gas_limit, &header.gas_limit, "gas_limit");
                check_eq(&exp.gas_used, &header.gas_used, "gas_used");
                check_eq(&exp.timestamp, &header.timestamp, "timestamp");
                check_eq(&exp.mix_hash, &header.mix_hash, "mix_hash");
                check_eq(&exp.nonce, &header.nonce, "nonce");
                check_eq(
                    &exp.base_fee_per_gas,
                    &header.base_fee_per_gas,
                    "base_fee_per_gas",
                );
                check_eq(&exp.blob_gas_used, &header.blob_gas_used, "blob_gas_used");
                check_eq(
                    &exp.excess_blob_gas,
                    &header.excess_blob_gas,
                    "excess_blob_gas",
                );
                check_eq(
                    &exp.parent_beacon_block_root,
                    &header.parent_beacon_block_root,
                    "parent_beacon_block_root",
                );
                check_eq(
                    &exp.extra_data.clone(),
                    &header.extra_data.clone(),
                    "extra_data",
                );

                // Make sure the blockhash from the node matches the one from the builder
                assert_eq!(
                    Into::<FixedBytes<32>>::into(header.hash().0),
                    input.block_hash_reference,
                    "block hash unexpected for block {}",
                    input.block_number,
                );
                let output = GuestOutput::Success { header, hash: pi };

                Ok(output)
            }
            Err(e) => {
                warn!("Proving bad block construction!");
                Err(HostError::Guest(
                    raiko_lib::prover::ProverError::GuestError(e.to_string()),
                ))
            }
        }
    }

    pub async fn prove(&self, input: GuestInput, output: &GuestOutput) -> HostResult<Proof> {
        self.request
            .proof_type
            .run_prover(
                input.clone(),
                output,
                &serde_json::to_value(self.request.clone())?,
            )
            .await
    }
}

pub struct NativeProver;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeParam {
    pub save_test_input: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeResponse {
    pub output: GuestOutput,
}

impl Prover for NativeProver {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        request: &serde_json::Value,
    ) -> ProverResult<Proof> {
        trace!("Running the native prover for input {input:?}");
        // Write the input.
        let param = NativeParam::deserialize(request.get("native").unwrap()).unwrap();
        if param.save_test_input {
            seriailize_input(&input, "./provers/sp1/contracts/src/fixtures/input.json");
        }

        let GuestOutput::Success { header, .. } = output.clone() else {
            return Err(ProverError::GuestError("Unexpected output".to_owned()));
        };

        ProtocolInstance::new(&input, &header, VerifierType::None)
            .map_err(|e| ProverError::GuestError(e.to_string()))?;

        to_proof(Ok(NativeResponse {
            output: output.clone(),
        }))
    }
}

fn seriailize_input(input: &GuestInput, path: &str) {
    let input = serde_json::to_string(&input).expect("Sp1: serializing input failed");
    std::fs::write(path, input).expect("failed to write input");
}

fn check_eq<T: std::cmp::PartialEq + std::fmt::Debug>(expected: &T, actual: &T, message: &str) {
    if expected != actual {
        error!("Assertion failed: {message} - Expected: {expected:?}, Found: {actual:?}");
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        interfaces::request::{ProofRequest, ProofType},
        provider::rpc::RpcBlockDataProvider,
        raiko::{ChainSpec, Raiko},
    };
    use alloy_primitives::Address;
    use clap::ValueEnum;
    use raiko_lib::consts::{Network, SupportedChainSpecs};
    use raiko_primitives::B256;
    use serde_json::{json, Value};
    use std::{collections::HashMap, env};

    fn get_proof_type_from_env() -> ProofType {
        let proof_type = env::var("TARGET").unwrap_or("native".to_string());
        ProofType::from_str(&proof_type, true).unwrap()
    }

    fn is_ci() -> bool {
        let ci = env::var("CI").unwrap_or("0".to_string());
        ci == "1"
    }

    fn test_proof_params() -> HashMap<String, Value> {
        let mut prover_args = HashMap::new();
        prover_args.insert(
            "risc0".to_string(),
            json! {
                {
                    "bonsai": false,
                    "snark": false,
                    "profile": true,
                    "execution_po2": 18
                }
            },
        );
        prover_args.insert(
            "sgx".to_string(),
            json! {
                {
                    "instance_id": 121,
                    "setup": true,
                    "bootstrap": true,
                    "prove": true,
                }
            },
        );
        prover_args.insert(
            "sp1".to_string(),
            json! {
                {
                    "recursion": "core",
                    "prover": "mock",
                    "save_test_input": false,

                }
            },
        );
        prover_args
    }

    async fn prove_block(
        l1_chain_spec: ChainSpec,
        taiko_chain_spec: ChainSpec,
        proof_request: ProofRequest,
    ) {
        let provider =
            RpcBlockDataProvider::new(&taiko_chain_spec.rpc, proof_request.block_number - 1)
                .expect("Could not create RpcBlockDataProvider");
        let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request.clone());
        let mut input = raiko
            .generate_input(provider)
            .await
            .expect("input generation failed");
        if is_ci() && proof_request.proof_type == ProofType::Sp1 {
            input.taiko.skip_verify_blob = true;
        }
        let output = raiko.get_output(&input).expect("output generation failed");
        let _proof = raiko
            .prove(input, &output)
            .await
            .expect("proof generation failed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_taiko_a7() {
        let proof_type = get_proof_type_from_env();
        let l1_network = Network::Holesky.to_string();
        let network = Network::TaikoA7.to_string();
        // Give the CI an simpler block to test because it doesn't have enough memory.
        // Unfortunately that also means that kzg is not getting fully verified by CI.
        let block_number = if is_ci() { 105987 } else { 101368 };
        let taiko_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&network)
            .unwrap();
        let l1_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&l1_network)
            .unwrap();

        let proof_request = ProofRequest {
            block_number,
            network,
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            l1_network,
            proof_type,
            prover_args: test_proof_params(),
        };
        prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prove_block_ethereum() {
        let proof_type = get_proof_type_from_env();
        // Skip test on SP1 for now because it's too slow on CI
        if !(is_ci() && proof_type == ProofType::Sp1) {
            let network = Network::Ethereum.to_string();
            let l1_network = Network::Ethereum.to_string();
            let block_number = 19707175;
            let taiko_chain_spec = SupportedChainSpecs::default()
                .get_chain_spec(&network)
                .unwrap();
            let l1_chain_spec = SupportedChainSpecs::default()
                .get_chain_spec(&l1_network)
                .unwrap();
            let proof_request = ProofRequest {
                block_number,
                network,
                graffiti: B256::ZERO,
                prover: Address::ZERO,
                l1_network,
                proof_type,
                prover_args: test_proof_params(),
            };
            prove_block(l1_chain_spec, taiko_chain_spec, proof_request).await;
        }
    }
}
