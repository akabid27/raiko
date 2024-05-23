#![cfg(feature = "enable")]
use std::{env, path::PathBuf};

use alloy_sol_types::SolType;
use raiko_lib::{
    input::{GuestInput, GuestOutput, RawGuestOutput},
    prover::{to_proof, Proof, Prover, ProverConfig, ProverResult},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sha3::{self, Digest};
use sp1_sdk::{ProverClient, SP1Stdin};

pub const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");
pub const E2E_TEST_INPUT_PATH: &str = "./provers/sp1/contracts/src/fixtures/input.json";

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sp1Param {
    pub recursion: RecursionMode,
    pub prover: ProverMode,
    pub save_test_input: bool,
}

#[serde(rename_all = "lowercase")]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RecursionMode {
    /// The proof mode for an SP1 core proof.
    Core,
    /// The proof mode for a compressed proof.
    Compressed,
    /// The proof mode for a PlonK proof.
    Plonk,
    /// The proof mode for a Groth16 proof.
    Groth16,
}

#[serde(rename_all = "lowercase")]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProverMode {
    Mock,
    Local,
    Remote,
}

macro_rules! save_and_return {
    ($proof:ident) => {
        // Save the proof.
        let proof_dir = std::env::current_dir().expect("Sp1: dir error");
        $proof
            .save(
                proof_dir
                    .as_path()
                    .join("proof-with-io.json")
                    .to_str()
                    .unwrap(),
            )
            .expect("Sp1: saving proof failed");
        println!("Successfully generated and verified proof for the program!");
        return to_proof(Ok(Sp1Response {
            proof: serde_json::to_string(&$proof).unwrap(),
        }));
    };
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: String,
}

pub struct Sp1Prover;

impl Prover for Sp1Prover {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        // Write the input.
        let param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();
        if param.save_test_input {
            seriailize_input(&input, E2E_TEST_INPUT_PATH);
        }

        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = match param.prover {
            ProverMode::Mock => ProverClient::mock(),
            ProverMode::Local => ProverClient::local(),
            ProverMode::Remote => ProverClient::remote(),
        };

        let (pk, vk) = client.setup(ELF);

        match param.recursion {
            RecursionMode::Core => {
                let proof = client.prove(&pk, stdin).expect("Sp1: proving failed");
                save_and_return!(proof);
            }
            RecursionMode::Compressed => {
                let proof = client
                    .prove_compressed(&pk, stdin)
                    .expect("Sp1: proving failed");
                save_and_return!(proof);
            }
            RecursionMode::Plonk => {
                let proof = client.prove_plonk(&pk, stdin).expect("Sp1: proving failed");
                save_and_return!(proof);
            }
            RecursionMode::Groth16 => {
                let proof = client
                    .prove_groth16(&pk, stdin)
                    .expect("Sp1: proving failed");
                save_and_return!(proof);
            }
        };
    }
}

fn seriailize_input(input: &GuestInput, path: &str) {
    let input = serde_json::to_string(&input).expect("Sp1: serializing input failed");
    std::fs::write(path, input).expect("failed to write input");
}

#[cfg(test)]
mod test {
    use super::*;
    const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-guest");

    #[test]
    fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::new();
        let stdin = SP1Stdin::new();
        let (pk, vk) = client.setup(TEST_ELF);
        let proof = client.prove(&pk, stdin).expect("Sp1: proving failed");
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");
    }
}
