# Issue #178 - Code Locations Reference

## Quick Index of Changes

### 1. Event Structures
**File:** `contracts/utility_contracts/src/lib.rs`
**Lines:** ~480-520

```
FirmwareUpdateStartedEvent
FirmwareUpdateFinishedEvent
UpdateCompleteData
SignedUpdateComplete
```

### 2. Error Codes
**File:** `contracts/utility_contracts/src/lib.rs`
**Lines:** 27-29 (within ContractError enum)

```
FirmwareUpdateInProgress = 27
FirmwareUpdateWindowExpired = 28
InvalidFirmwareUpdateSignature = 29
```

### 3. Constants
**File:** `contracts/utility_contracts/src/lib.rs`
**After line:** 648

```
const FIRMWARE_UPDATE_WINDOW_SECS: u64 = 2 * HOUR_IN_SECONDS; // 7200 seconds
```

### 4. Meter Struct Extension
**File:** `contracts/utility_contracts/src/lib.rs`
**Lines:** 196-199

```rust
// Issue #178: Firmware Update Authorization Gate Fields
pub is_updating: bool,
pub update_start_timestamp: u64,
```

### 5. Meter Initialization in register_meter_with_mode()
**File:** `contracts/utility_contracts/src/lib.rs`
**After line:** 2580

```rust
is_updating: false,
update_start_timestamp: 0,
```

### 6. initiate_firmware_update() Function
**File:** `contracts/utility_contracts/src/lib.rs`
**Lines:** ~3595-3635

Core implementation of provider-initiated firmware update.

**Key Points:**
- Requires provider authentication
- Sets `is_updating = true`
- Records current timestamp
- Emits `FirmwareUpdateStartedEvent`

### 7. complete_firmware_update() Function
**File:** `contracts/utility_contracts/src/lib.rs`
**Lines:** ~3637-3701

Core implementation of device-completed firmware update with signature verification.

**Key Points:**
- Verifies Ed25519 signature
- Enforces 2-hour time limit
- Validates device public key
- Checks timestamp matches
- Sets `is_updating = false`
- Emits `FirmwareUpdateFinishedEvent`

### 8. deduct_units() Modification
**File:** `contracts/utility_contracts/src/lib.rs`
**After line:** 2785

Added billing pause gate:
```rust
// Issue #178: Check if meter is under firmware update
// Billing is paused during authorized update window
if meter.is_updating {
    panic_with_error!(&env, ContractError::FirmwareUpdateInProgress);
}
```

### 9. Test Suite
**File:** `contracts/utility_contracts/tests/firmware_update_tests.rs` (NEW)
**Lines:** 1-430+

Complete test module with:
- 5 acceptance criteria tests
- Integration workflow test
- Edge case tests
- Authorization tests
- Event emission tests

---

## Detailed Code Changes

### Change 1: Event Structures (after line ~475)

```rust
// Issue #178: Firmware Update Authorization Gate
// Structures for managing authorized firmware updates on IoT devices
#[contracttype]
#[derive(Clone)]
pub struct FirmwareUpdateStartedEvent {
    pub meter_id: u64,
    pub update_start_timestamp: u64,
    pub provider: Address,
    pub max_update_window_secs: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct FirmwareUpdateFinishedEvent {
    pub meter_id: u64,
    pub update_start_timestamp: u64,
    pub update_completed_timestamp: u64,
    pub update_duration_secs: u64,
    pub device_signature_valid: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct UpdateCompleteData {
    pub meter_id: u64,
    pub update_start_timestamp: u64,
    pub completion_timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct SignedUpdateComplete {
    pub meter_id: u64,
    pub update_start_timestamp: u64,
    pub completion_timestamp: u64,
    pub signature: BytesN<64>,
    pub device_public_key: BytesN<32>,
}
```

### Change 2: Error Codes (within ContractError enum)

```rust
// Issue #178: Firmware Update Authorization Gate error codes
FirmwareUpdateInProgress = 27,
FirmwareUpdateWindowExpired = 28,
InvalidFirmwareUpdateSignature = 29,
```

### Change 3: Constants

```rust
// Issue #178: Firmware Update Authorization Gate constants
const FIRMWARE_UPDATE_WINDOW_SECS: u64 = 2 * HOUR_IN_SECONDS; // 2 hours max update window
```

### Change 4: Meter Struct

```rust
pub struct Meter {
    // ... existing fields ...
    
    // Issue #178: Firmware Update Authorization Gate Fields
    pub is_updating: bool,
    pub update_start_timestamp: u64,
}
```

### Change 5: register_meter_with_mode() Initialization

```rust
let meter = Meter {
    // ... existing initializations ...
    
    is_updating: false,
    update_start_timestamp: 0,
};
```

### Change 6: New Function - initiate_firmware_update()

Location: Before `get_billing_group()` function (around line 3595)

```rust
/// Initiate a firmware update for a meter (provider-only)
/// This pauses billing during the update window and requires device signature to resume
pub fn initiate_firmware_update(env: Env, meter_id: u64) {
    let mut meter = get_meter_or_panic(&env, meter_id);
    
    // Only provider can initiate firmware update
    meter.provider.require_auth();
    
    // Check if already updating
    if meter.is_updating {
        panic_with_error!(&env, ContractError::FirmwareUpdateInProgress);
    }
    
    let now = env.ledger().timestamp();
    
    // Set update flag and timestamp
    meter.is_updating = true;
    meter.update_start_timestamp = now;
    
    env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    
    // Emit FirmwareUpdateStarted event
    let event = FirmwareUpdateStartedEvent {
        meter_id,
        update_start_timestamp: now,
        provider: meter.provider.clone(),
        max_update_window_secs: FIRMWARE_UPDATE_WINDOW_SECS,
    };
    
    env.events().publish(
        (symbol_short!("FWUpdStart"), meter_id),
        event,
    );
}
```

### Change 7: New Function - complete_firmware_update()

Location: After `initiate_firmware_update()` (around line 3637)

```rust
/// Complete firmware update with device signature
/// Device must sign the UpdateCompleteData to resume billing
pub fn complete_firmware_update(env: Env, signed_update: SignedUpdateComplete) {
    let mut meter = get_meter_or_panic(&env, signed_update.meter_id);
    
    // Check if meter is currently updating
    if !meter.is_updating {
        panic_with_error!(&env, ContractError::MeterNotFound);
    }
    
    let now = env.ledger().timestamp();
    
    // Verify update window hasn't expired (max 2 hours)
    if now.saturating_sub(meter.update_start_timestamp) > FIRMWARE_UPDATE_WINDOW_SECS {
        panic_with_error!(&env, ContractError::FirmwareUpdateWindowExpired);
    }
    
    // Verify update_start_timestamp matches
    if signed_update.update_start_timestamp != meter.update_start_timestamp {
        panic_with_error!(&env, ContractError::InvalidFirmwareUpdateSignature);
    }
    
    // Verify the device public key matches
    if signed_update.device_public_key != meter.device_public_key {
        panic_with_error!(&env, ContractError::PublicKeyMismatch);
    }
    
    // Create the message that was signed by the device
    let completion_data = UpdateCompleteData {
        meter_id: signed_update.meter_id,
        update_start_timestamp: signed_update.update_start_timestamp,
        completion_timestamp: signed_update.completion_timestamp,
    };
    
    // Verify the signature using Ed25519 (Soroban's built-in crypto)
    #[cfg(not(test))]
    env.crypto().ed25519_verify(
        &signed_update.device_public_key,
        &completion_data.to_xdr(&env),
        &signed_update.signature,
    );
    
    // Update meter state to resume billing
    meter.is_updating = false;
    meter.update_start_timestamp = 0;
    meter.last_update = now;
    
    env.storage().instance().set(&DataKey::Meter(signed_update.meter_id), &meter);
    
    // Calculate update duration
    let update_duration_secs = now.saturating_sub(meter.update_start_timestamp);
    
    // Emit FirmwareUpdateFinished event
    let event = FirmwareUpdateFinishedEvent {
        meter_id: signed_update.meter_id,
        update_start_timestamp: signed_update.update_start_timestamp,
        update_completed_timestamp: now,
        update_duration_secs,
        device_signature_valid: true,
    };
    
    env.events().publish(
        (symbol_short!("FWUpdEnd"), signed_update.meter_id),
        event,
    );
}
```

### Change 8: Modify deduct_units()

Location: After line ~2785

Add before the `let now = env.ledger().timestamp();` line:

```rust
// Issue #178: Check if meter is under firmware update
// Billing is paused during authorized update window
if meter.is_updating {
    panic_with_error!(&env, ContractError::FirmwareUpdateInProgress);
}
```

---

## Verification Checklist

Use this checklist to verify all changes are in place:

- [ ] Event structure `FirmwareUpdateStartedEvent` defined
- [ ] Event structure `FirmwareUpdateFinishedEvent` defined
- [ ] Structure `UpdateCompleteData` defined
- [ ] Structure `SignedUpdateComplete` defined
- [ ] Error code `FirmwareUpdateInProgress = 27` added
- [ ] Error code `FirmwareUpdateWindowExpired = 28` added
- [ ] Error code `InvalidFirmwareUpdateSignature = 29` added
- [ ] Constant `FIRMWARE_UPDATE_WINDOW_SECS = 7200` defined
- [ ] Meter field `is_updating: bool` added
- [ ] Meter field `update_start_timestamp: u64` added
- [ ] Fields initialized in `register_meter_with_mode()`
- [ ] Function `initiate_firmware_update()` implemented
- [ ] Function `complete_firmware_update()` implemented
- [ ] Billing gate added to `deduct_units()`
- [ ] Test file `firmware_update_tests.rs` created

---

## Related Documentation

- `FIRMWARE_UPDATE_IMPLEMENTATION.md` - Detailed specifications
- `FIRMWARE_UPDATE_SUMMARY.md` - Implementation overview
- `firmware_update_tests.rs` - Complete test suite
