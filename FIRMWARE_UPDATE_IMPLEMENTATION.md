# Issue #178: Firmware-Update Authorization Gate Implementation

## Overview
This document describes the implementation of the Firmware-Update Authorization Gate feature for Utility-Drip-Contracts. This feature enables secure, time-limited firmware updates on IoT devices while protecting against billing manipulation during the update window.

## Problem Statement
IoT devices require periodic firmware updates for security and functionality improvements. However, firmware updates create a unique billing challenge:
- Devices cannot accurately report usage during updates
- Providers should prevent billing during update windows to avoid inaccurate charges
- Without controls, devices could remain in "updating" state indefinitely to avoid billing

## Solution Design

### Architecture Overview
The firmware update authorization gate implements three key protections:

1. **Billing Pause During Update**: When a provider initiates a firmware update, the meter's billing is automatically suspended using the `is_updating` flag
2. **Time Limit Enforcement**: Updates are limited to a maximum 2-hour window to prevent perpetual suspension
3. **Cryptographic Proof of Completion**: Only devices with a valid Ed25519 signature matching the device's public key can resume billing

### Data Structures

#### Meter Struct Extensions
```rust
pub struct Meter {
    // ... existing fields ...
    
    // Issue #178: Firmware Update Authorization Gate Fields
    pub is_updating: bool,              // Flag indicating device is under firmware update
    pub update_start_timestamp: u64,    // Timestamp when update was initiated (seconds)
}
```

#### Event Structures
```rust
pub struct FirmwareUpdateStartedEvent {
    pub meter_id: u64,                  // Meter identifier
    pub update_start_timestamp: u64,    // Timestamp when update began
    pub provider: Address,              // Provider who initiated update
    pub max_update_window_secs: u64,    // Maximum allowed update duration (7200 = 2 hours)
}

pub struct FirmwareUpdateFinishedEvent {
    pub meter_id: u64,                  // Meter identifier
    pub update_start_timestamp: u64,    // Original start timestamp
    pub update_completed_timestamp: u64,// When update completed
    pub update_duration_secs: u64,      // Actual duration of update
    pub device_signature_valid: bool,   // Whether signature verification succeeded
}
```

#### Update Completion Structures
```rust
pub struct UpdateCompleteData {
    pub meter_id: u64,                  // Meter being updated
    pub update_start_timestamp: u64,    // Must match meter's registered start time
    pub completion_timestamp: u64,      // When device completed update
}

pub struct SignedUpdateComplete {
    pub meter_id: u64,
    pub update_start_timestamp: u64,
    pub completion_timestamp: u64,
    pub signature: BytesN<64>,          // Ed25519 signature (64 bytes)
    pub device_public_key: BytesN<32>,  // Device's public key (32 bytes)
}
```

### Constants
```rust
const FIRMWARE_UPDATE_WINDOW_SECS: u64 = 2 * HOUR_IN_SECONDS; // 7200 seconds (2 hours)
const HOUR_IN_SECONDS: u64 = 60 * 60; // 3600 seconds
```

### Error Codes
```rust
pub enum ContractError {
    // ... existing errors ...
    FirmwareUpdateInProgress = 27,      // Meter is currently updating, billing paused
    FirmwareUpdateWindowExpired = 28,   // Update window exceeded (> 2 hours)
    InvalidFirmwareUpdateSignature = 29,// Device signature verification failed
}
```

## Function Specifications

### 1. initiate_firmware_update(meter_id: u64)

**Authorization**: Provider-only (requires provider authentication)

**Purpose**: Initiates a firmware update for a meter and suspends billing

**Parameters**:
- `meter_id`: The meter to update

**Behavior**:
1. Authenticates caller as the meter's provider
2. Checks if meter is already updating (error: `FirmwareUpdateInProgress`)
3. Sets `is_updating = true`
4. Sets `update_start_timestamp = current_time`
5. Stores updated meter state
6. Emits `FirmwareUpdateStartedEvent`

**Returns**: None

**Error Conditions**:
- `ContractError::FirmwareUpdateInProgress`: Meter already under update
- `ContractError::Unauthorized`: Caller is not the provider

### 2. complete_firmware_update(signed_update: SignedUpdateComplete)

**Authorization**: Device holder (via cryptographic proof)

**Purpose**: Completes firmware update and resumes billing with cryptographic proof

**Parameters**:
- `signed_update`: `SignedUpdateComplete` struct containing:
  - meter_id
  - update_start_timestamp
  - completion_timestamp
  - signature (Ed25519, 64 bytes)
  - device_public_key (32 bytes)

**Behavior**:
1. Retrieves meter; checks if currently updating
2. Verifies update window hasn't expired:
   - Current time - update_start_timestamp ≤ 7200 seconds (2 hours)
3. Verifies update_start_timestamp matches meter's timestamp
4. Verifies device_public_key matches meter's registered device_public_key
5. Verifies Ed25519 signature of UpdateCompleteData
6. Sets `is_updating = false`
7. Clears `update_start_timestamp = 0`
8. Updates `last_update = current_time`
9. Stores updated meter state
10. Emits `FirmwareUpdateFinishedEvent`

**Returns**: None

**Error Conditions**:
- `ContractError::MeterNotFound`: Meter not updating or doesn't exist
- `ContractError::FirmwareUpdateWindowExpired`: Update duration > 2 hours
- `ContractError::PublicKeyMismatch`: Device public key doesn't match
- `ContractError::InvalidFirmwareUpdateSignature`: Timestamp mismatch or invalid signature

### 3. deduct_units() - Modified Behavior

**New Gate**: Billing pause check added

Lines added (after existing checks):
```rust
// Issue #178: Check if meter is under firmware update
// Billing is paused during authorized update window
if meter.is_updating {
    panic_with_error!(&env, ContractError::FirmwareUpdateInProgress);
}
```

**Behavior Change**:
- `deduct_units()` now rejects with `FirmwareUpdateInProgress` if meter is updating
- This ensures no usage charges accrue during the update window

## Acceptance Criteria Mapping

### Acceptance 1: Billing pauses precisely during authorized update window ✓
- **Implementation**: 
  - `is_updating` flag added to Meter struct
  - `initiate_firmware_update()` sets flag when called
  - `deduct_units()` checks flag and rejects if true
  - `complete_firmware_update()` clears flag to resume
- **Verification**: Test `test_firmware_update_acceptance_1_billing_pauses_during_window`

### Acceptance 2: Time limits prevent perpetual suspension ✓
- **Implementation**:
  - `complete_firmware_update()` enforces 2-hour maximum window
  - Window calculated: `now - update_start_timestamp > FIRMWARE_UPDATE_WINDOW_SECS`
  - Returns `FirmwareUpdateWindowExpired` error if exceeded
- **Verification**: Test `test_firmware_update_acceptance_2_time_limits_prevent_perpetual_suspension`

### Acceptance 3: Hardware cryptographic signatures required to resume ✓
- **Implementation**:
  - `complete_firmware_update()` requires valid Ed25519 signature
  - Signature verified against `device_public_key` registered with meter
  - Uses Soroban's `env.crypto().ed25519_verify()`
  - Signature must be exactly 64 bytes
  - Public key must be exactly 32 bytes
- **Verification**: Test `test_firmware_update_acceptance_3_hardware_signatures_required`

## Security Considerations

### 1. Signature Verification
- Uses Ed25519 algorithm (industry standard for IoT)
- Signature is 64 bytes, public key is 32 bytes
- Message contains: meter_id, update_start_timestamp, completion_timestamp
- Prevents unauthorized devices from resuming billing

### 2. Replay Attack Prevention
- Each UpdateComplete must include the exact `update_start_timestamp`
- Prevents old signatures from being reused
- Timestamp mismatch results in `InvalidFirmwareUpdateSignature`

### 3. Time Window Protection
- 2-hour maximum prevents indefinite billing suspension
- Provider cannot extend window; only device can resume
- Expired windows prevent completion with any signature

### 4. Authorization
- Only provider can initiate updates (requires auth)
- Only device with matching public key can complete updates
- No administrative override for expired windows

## Event Emission

### FirmwareUpdateStartedEvent (Symbol: "FWUpdStart")
Emitted when: `initiate_firmware_update()` succeeds
Properties:
- meter_id: u64
- update_start_timestamp: u64
- provider: Address
- max_update_window_secs: u64

### FirmwareUpdateFinishedEvent (Symbol: "FWUpdEnd")
Emitted when: `complete_firmware_update()` succeeds
Properties:
- meter_id: u64
- update_start_timestamp: u64
- update_completed_timestamp: u64
- update_duration_secs: u64
- device_signature_valid: bool

## Testing

### Test Coverage

1. **Acceptance Criteria Tests**
   - `test_firmware_update_acceptance_1_billing_pauses_during_window`
   - `test_firmware_update_acceptance_2_time_limits_prevent_perpetual_suspension`
   - `test_firmware_update_acceptance_3_hardware_signatures_required`

2. **Integration Tests**
   - `test_firmware_update_integration_workflow` - Full workflow from start to completion

3. **Edge Case Tests**
   - Multiple consecutive update attempts
   - Window expiration at boundary (7200 seconds)
   - Signature with wrong timestamp
   - Public key mismatch

4. **Authorization Tests**
   - Provider-only authorization for `initiate_firmware_update()`
   - Device signature requirement for `complete_firmware_update()`

5. **Event Emission Tests**
   - Proper event emission with correct fields
   - Event symbol verification

### Running Tests
```bash
# Run unit tests
cargo test -p utility_contracts --lib firmware_update

# Run integration tests
cargo test --test firmware_update_tests

# Run all tests with output
cargo test -p utility_contracts -- --nocapture
```

## Usage Example

### Provider Initiates Update
```
Provider calls: initiate_firmware_update(meter_id=123)
Result: 
- is_updating = true
- update_start_timestamp = current_time
- Billing paused, deduct_units() will reject
- FirmwareUpdateStartedEvent emitted
```

### Device Completes Update
```
Device calls: complete_firmware_update({
  meter_id: 123,
  update_start_timestamp: 1000,
  completion_timestamp: 1600,
  signature: [ed25519_signature_64_bytes],
  device_public_key: [public_key_32_bytes]
})

Steps:
1. Verifies device_public_key matches meter's registered key
2. Checks 1600 - 1000 = 600 seconds (within 7200 limit) ✓
3. Verifies Ed25519 signature
4. Sets is_updating = false
5. Emits FirmwareUpdateFinishedEvent with 600 second duration
```

### Billing Resumes
```
Now deduct_units() succeeds because:
- is_updating = false
- Update billing continues normally
```

## Implementation Status

✓ Meter struct extended with firmware update fields
✓ Event structures defined (FirmwareUpdateStartedEvent, FirmwareUpdateFinishedEvent)
✓ Error codes added (FirmwareUpdateInProgress, FirmwareUpdateWindowExpired, InvalidFirmwareUpdateSignature)
✓ initiate_firmware_update() function implemented
✓ complete_firmware_update() function with signature verification
✓ deduct_units() modified to enforce update pause
✓ Constants defined (FIRMWARE_UPDATE_WINDOW_SECS = 7200)
✓ Comprehensive test suite created
✓ Documentation completed

## File Changes

### Modified Files
1. `contracts/utility_contracts/src/lib.rs`
   - Added event structures
   - Added error codes
   - Extended Meter struct
   - Updated register_meter_with_mode()
   - Added initiate_firmware_update()
   - Added complete_firmware_update()
   - Modified deduct_units()
   - Added constant FIRMWARE_UPDATE_WINDOW_SECS

### New Files
1. `contracts/utility_contracts/tests/firmware_update_tests.rs`
   - Comprehensive test suite with acceptance criteria tests

## References

- **Issue**: #178 - Firmware-Update Authorization Gate
- **Labels**: iot, maintenance, state-machine
- **Soroban Crypto**: https://github.com/stellar/rs-soroban-sdk
- **Ed25519 Signatures**: https://en.wikipedia.org/wiki/EdDSA

## Future Enhancements

1. Add optional firmware version tracking
2. Support multiple concurrent updates (per component)
3. Automatic rollback on extended offline state
4. Update progress reporting (percentage complete)
5. Multiple device support per meter
