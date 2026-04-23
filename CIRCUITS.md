# ZK-SNARK Circuits for Sensor Privacy

This document outlines the design and implementation of the ZK-SNARK circuits used by the Utility-Drip system to preserve sensor data privacy.

## Overview

The goal is to allow a hardware device (meter) to prove it has consumed a specific amount of energy/water without revealing the raw, granular sensor readings. The contract verifies this proof and deducts the appropriate balance.

## Circuit Specification (Circom)

The circuit is implemented in [Circom](https://iden3.io/circom) and uses the [Groth16](https://eprint.iacr.org/2016/260.pdf) proof system.

### Private Inputs (Witness)

- `usage_raw`: The raw, high-precision sensor reading.
- `salt`: A random salt to ensure commitment privacy.
- `last_usage`: The previous raw reading stored locally on the device.

### Public Inputs

- `units_consumed`: The calculated units to be billed (e.g., `(usage_raw - last_usage) * rate`).
- `is_peak_hour`: Whether the current time is a peak hour.
- `nullifier`: A unique value to prevent proof replay.
- `commitment`: A hash of the current state.

### Constraints

1.  **Integrity**: `units_consumed` must be correctly calculated from the change in raw usage.
2.  **Range Proof**: `units_consumed` must be within a valid range (e.g., `< 1,000,000`).
3.  **Commitment**: `commitment == Poseidon(usage_raw, salt)`.
4.  **Nullifier**: `nullifier == Poseidon(last_usage, salt)`.

## Proving & Verification Flow

1.  **Hardware Device**:
    - Reads sensor data.
    - Generates a Groth16 proof using the local witness.
    - Submits the proof and public inputs to the contract via `submit_zk_usage_report`.
2.  **Smart Contract**:
    - Uses native Soroban BN254 host functions (`pairing_check`, `g1_add`, `g1_mul`) to verify the proof.
    - Verifies the `nullifier` hasn't been used before.
    - Deducts the balance based on the verified `units_consumed`.

## Optimization for Soroban

To stay within the ledger's instruction limits, the verifier is optimized by:
- Using pre-computed components in the Verification Key.
- Utilizing optimized host functions for all elliptic curve operations.
- Avoiding expensive big-integer arithmetic in WASM guest code.

## Key Files

- `contracts/utility_contracts/src/lib.rs`: Contains the `verify_groth16_proof` logic.
- `meter-simulator/src/meter-device.js`: Simulates the proving process for testing.

## Deployment

1.  Generate the Verification Key (`verification_key.json`) using `snarkjs`.
2.  Format the key for Soroban (Big-Endian bytes).
3.  Call `set_zk_verification_key` on the contract to register the key for your meter.
