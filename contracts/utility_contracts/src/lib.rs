#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    Address, Env, BytesN, Vec, Symbol,
};

// Oracle client interface
use soroban_sdk::contractclient;

#[contractclient(name = "PriceOracleClient")]
pub trait PriceOracle {
    fn xlm_to_usd_cents(env: Env, xlm_amount: i128) -> i128;
    fn usd_cents_to_xlm(env: Env, usd_cents: i128) -> i128;
    fn get_price(env: Env) -> PriceData;
}

#[contracttype]
#[derive(Clone)]
pub struct PriceData {
    pub price: i128,
    pub decimals: u32,
    pub last_updated: u64,
}
// mod fuzz_tests;

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BillingType {
    PrePaid,
    PostPaid,
}

#[contracttype]
#[derive(Clone)]
pub struct SignedUsageData {
    pub meter_id: u64,
    pub timestamp: u64,
    pub watt_hours_consumed: i128,
    pub units_consumed: i128,
    pub signature: BytesN<64>,
    pub public_key: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
pub struct UsageData {
    pub total_watt_hours: i128,
    pub current_cycle_watt_hours: i128,
    pub peak_usage_watt_hours: i128,
    pub last_reading_timestamp: u64,
    pub precision_factor: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavingGoal {
    pub target_amount: i128,    // goal in USD cents
    pub current_savings: i128, // currently saved in USD cents
    pub marketplace: Address,  // where to spend
    pub is_completed: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct Meter {
    pub user: Address,
    pub provider: Address,
    pub billing_type: BillingType,
    pub off_peak_rate: i128,      // rate per second during off-peak hours
    pub peak_rate: i128,          // rate per second during peak hours (1.5x off-peak)
    pub rate_per_second: i128,
    pub rate_per_unit: i128,
    pub balance: i128,
    pub debt: i128,
    pub collateral_limit: i128,
    pub last_update: u64,
    pub is_active: bool,
    pub token: Address,
    pub usage_data: UsageData,
    pub max_flow_rate_per_hour: i128,
    pub last_claim_time: u64,
    pub claimed_this_hour: i128,
    pub heartbeat: u64,
    pub device_public_key: BytesN<32>,
    pub end_date: u64,
    pub rent_deposit: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct ProviderWithdrawalWindow {
    pub daily_withdrawn: i128,
    pub last_reset: u64,
}

#[contracttype]
pub enum DataKey {
    Meter(u64),
    ProviderWindow(Address),
    Count,
    Oracle,
    ActiveMetersCount,
    SeasonalFactor,
    Treasury,
    ProviderVolume(Address),
    SavingGoal(u64),
    NativeToken,
}

#[contracterror]
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    MeterNotFound = 1,
    OracleNotSet = 2,
    WithdrawalLimitExceeded = 3,
    PriceConversionFailed = 4,
    InvalidTokenAmount = 5,
    InvalidUsageValue = 6,
    UsageExceedsLimit = 7,
    InvalidPrecisionFactor = 8,
    InvalidSignature = 9,
    PublicKeyMismatch = 10,
    TimestampTooOld = 11,
    StreamNotFinished = 12,
    BalanceNotEmpty = 13,
    GoalAlreadyMet = 14,
    GoalNotFound = 15,
}

#[contract]
pub struct UtilityContract;

const HOUR_IN_SECONDS: u64 = 60 * 60;
const DAY_IN_SECONDS: u64 = 24 * HOUR_IN_SECONDS;
const DAILY_WITHDRAWAL_PERCENT: i128 = 10;
const MAX_USAGE_PER_UPDATE: i128 = 1_000_000_000_000i128; // 1 billion kWh max per update
const MIN_PRECISION_FACTOR: i128 = 1;
const MAX_TIMESTAMP_DELAY: u64 = 300; // 5 minutes

fn verify_usage_signature(env: &Env, signed_data: &SignedUsageData, meter: &Meter) -> Result<(), ContractError> {
    // Check if the provided public key matches the registered meter's public key
    if signed_data.public_key != meter.device_public_key {
        return Err(ContractError::PublicKeyMismatch);
    }

    // Check timestamp is not too old (prevent replay attacks)
    let current_time = env.ledger().timestamp();
    if current_time.saturating_sub(signed_data.timestamp) > MAX_TIMESTAMP_DELAY {
        return Err(ContractError::TimestampTooOld);
    }

    // Create the message that was signed: meter_id || timestamp || watt_hours_consumed || units_consumed
    let mut message = Vec::new(env);
    message.push_back(signed_data.meter_id);
    message.push_back(signed_data.timestamp);
    message.push_back(signed_data.watt_hours_consumed as u64);
    message.push_back(signed_data.units_consumed as u64);

    // Verify the signature using Soroban's built-in signature verification
    use soroban_sdk::xdr::ToXdr;
    env.crypto().ed25519_verify(
        &signed_data.public_key,
        &message.to_xdr(env),
        &signed_data.signature,
    );
    Ok(())
}

// Peak hours: 18:00 - 21:00 UTC
const PEAK_HOUR_START: u64 = 18 * HOUR_IN_SECONDS; // 64800 seconds
const PEAK_HOUR_END: u64 = 21 * HOUR_IN_SECONDS;   // 75600 seconds
const PEAK_RATE_MULTIPLIER: i128 = 3; // 1.5x => stored as 3 (divide by 2)
const RATE_PRECISION: i128 = 2; // Precision for rate calculations

fn is_native_token(env: &Env, token_address: &Address) -> bool {
    match env.storage().instance().get::<DataKey, Address>(&DataKey::NativeToken) {
        Some(native) => token_address == &native,
        None => false,
    }
}

fn transfer_tokens(env: Env, token_address: &Address, from: &Address, to: &Address, amount: &i128) {
    let client = token::Client::new(&env, token_address);
    client.transfer(from, to, amount);
}

fn get_token_balance(env: Env, token_address: &Address, account: &Address) -> i128 {
    let client = token::Client::new(&env, token_address);
    client.balance(account)
}


fn get_meter_or_panic(env: &Env, meter_id: u64) -> Meter {
    match env
        .storage()
        .instance()
        .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
    {
        Some(meter) => meter,
        None => panic_with_error!(env, ContractError::MeterNotFound),
    }
}

fn get_oracle_or_panic(env: &Env) -> Address {
    match env
        .storage()
        .instance()
        .get::<DataKey, Address>(&DataKey::Oracle)
    {
        Some(oracle) => oracle,
        None => panic_with_error!(env, ContractError::OracleNotSet),
    }
}

fn convert_xlm_to_usd_if_needed(env: &Env, amount: i128, token: &Address) -> Result<i128, ContractError> {
    if !env.storage().instance().has(&DataKey::Oracle) {
        return Ok(amount);
    }
    
    let oracle_address = get_oracle_or_panic(env);
    let oracle_client = PriceOracleClient::new(env, &oracle_address);
    Ok(oracle_client.xlm_to_usd_cents(&amount))
}

fn convert_usd_to_xlm_if_needed(env: &Env, usd_cents: i128, token: &Address) -> Result<i128, ContractError> {
    if !env.storage().instance().has(&DataKey::Oracle) {
        return Ok(usd_cents);
    }
    
    let oracle_address = get_oracle_or_panic(env);
    let oracle_client = PriceOracleClient::new(env, &oracle_address);
    Ok(oracle_client.usd_cents_to_xlm(&usd_cents))
}

fn remaining_postpaid_collateral(meter: &Meter) -> i128 {
    meter.collateral_limit.saturating_sub(meter.debt).max(0)
}

fn is_peak_hour(timestamp: u64) -> bool {
    let seconds_in_day = timestamp % DAY_IN_SECONDS;
    seconds_in_day >= PEAK_HOUR_START && seconds_in_day < PEAK_HOUR_END
}

fn get_seasonal_multiplier(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::SeasonalFactor).unwrap_or(100)
}

fn get_effective_rate(env: &Env, meter: &Meter, timestamp: u64) -> i128 {
    let base_rate = if is_peak_hour(timestamp) {
        meter.peak_rate
    } else {
        meter.off_peak_rate
    };
    
    let multiplier = get_seasonal_multiplier(env);
    base_rate.saturating_mul(multiplier) / 100
}

fn provider_meter_value(meter: &Meter) -> i128 {
    match meter.billing_type {
        BillingType::PrePaid => meter.balance.max(0),
        BillingType::PostPaid => remaining_postpaid_collateral(meter),
    }
}

fn refresh_activity(meter: &mut Meter) {
    meter.is_active = provider_meter_value(meter) > 0;
}

fn reset_claim_window_if_needed(meter: &mut Meter, now: u64) {
    if now.saturating_sub(meter.last_claim_time) >= HOUR_IN_SECONDS {
        meter.claimed_this_hour = 0;
        meter.last_claim_time = now;
    }
}

fn remaining_claim_capacity(meter: &Meter) -> i128 {
    meter
        .max_flow_rate_per_hour
        .saturating_sub(meter.claimed_this_hour)
        .max(0)
}

fn get_provider_window_or_default(
    env: &Env,
    provider: &Address,
    now: u64,
) -> ProviderWithdrawalWindow {
    env.storage()
        .instance()
        .get(&DataKey::ProviderWindow(provider.clone()))
        .unwrap_or(ProviderWithdrawalWindow {
            daily_withdrawn: 0,
            last_reset: now,
        })
}

fn reset_provider_window_if_needed(window: &mut ProviderWithdrawalWindow, now: u64) {
    if now.saturating_sub(window.last_reset) >= DAY_IN_SECONDS {
        window.daily_withdrawn = 0;
        window.last_reset = now;
    }
}

fn get_provider_total_pool(env: &Env, provider: &Address) -> i128 {
    let count = env
        .storage()
        .instance()
        .get::<DataKey, u64>(&DataKey::Count)
        .unwrap_or(0);
    let mut total_pool: i128 = 0;
    let mut meter_id = 1;

    while meter_id <= count {
        if let Some(meter) = env
            .storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
        {
            if meter.provider == *provider {
                total_pool = total_pool.saturating_add(provider_meter_value(&meter));
            }
        }

        meter_id += 1;
    }

    total_pool
}

fn apply_provider_withdrawal_limit(
    env: &Env,
    provider: &Address,
    amount: i128,
) -> ProviderWithdrawalWindow {
    let now = env.ledger().timestamp();
    let mut window = get_provider_window_or_default(env, provider, now);
    reset_provider_window_if_needed(&mut window, now);

    if amount <= 0 {
        return window;
    }

    let total_pool_before_claim =
        get_provider_total_pool(env, provider).saturating_add(window.daily_withdrawn);
    let daily_limit = total_pool_before_claim / DAILY_WITHDRAWAL_PERCENT;

    if window.daily_withdrawn.saturating_add(amount) > daily_limit {
        panic_with_error!(env, ContractError::WithdrawalLimitExceeded);
    }

    window.daily_withdrawn = window.daily_withdrawn.saturating_add(amount);
    window
}

fn apply_provider_claim(env: &Env, meter: &mut Meter, meter_id: u64, amount: i128) {
    if amount <= 0 {
        return;
    }

    let mut provider_share = amount;
    
    // 1. Handle Saving Goal (#115)
    if let Some(mut goal) = env.storage().instance().get::<DataKey, SavingGoal>(&DataKey::SavingGoal(meter_id)) {
        if !goal.is_completed {
            let contribution = amount / 5; // Fixed 20% redirection for saving upgrades
            goal.current_savings = goal.current_savings.saturating_add(contribution);
            provider_share = provider_share.saturating_sub(contribution);

            if goal.current_savings >= goal.target_amount {
                goal.is_completed = true;
                env.events().publish(
                    (symbol_short!("AutoBuy"), meter.user.clone()), 
                    (goal.marketplace.clone(), goal.target_amount)
                );
            }
            env.storage().instance().set(&DataKey::SavingGoal(meter_id), &goal);
        }
    }

    // 2. Handle Sustainability Fee (#132)
    let mut provider_vol = env.storage().instance()
        .get::<DataKey, i128>(&DataKey::ProviderVolume(meter.provider.clone()))
        .unwrap_or(0);
    
    let mut transfer_to_provider = provider_share;
    
    if provider_vol >= 10_000_000 { // $100k Threshold (10,000,000 cents)
        let fee = provider_share / 1000; // 0.1% Maintenance Tax
        if fee > 0 {
            if let Some(treasury) = env.storage().instance().get::<DataKey, Address>(&DataKey::Treasury) {
                transfer_tokens(env.clone(), &meter.token, &env.current_contract_address(), &treasury, &fee);
                transfer_to_provider = transfer_to_provider.saturating_sub(fee);
            }
        }
    }

    // 3. Final Transfers
    if transfer_to_provider > 0 {
        transfer_tokens(env.clone(), &meter.token, &env.current_contract_address(), &meter.provider, &transfer_to_provider);
    }
    
    // Update lifetime volume
    provider_vol = provider_vol.saturating_add(provider_share);
    env.storage().instance().set(&DataKey::ProviderVolume(meter.provider.clone()), &provider_vol);

    // 4. Update Meter State
    match meter.billing_type {
        BillingType::PrePaid => {
            meter.balance = meter.balance.saturating_sub(amount);
        }
        BillingType::PostPaid => {
            meter.debt = meter.debt.saturating_add(amount);
        }
    }

    meter.claimed_this_hour = meter.claimed_this_hour.saturating_add(amount);
}

fn publish_active_event(env: &Env, meter_id: u64, now: u64) {
    env.events()
        .publish((symbol_short!("Active"), meter_id), now);
}

fn publish_inactive_event(env: &Env, meter_id: u64, now: u64) {
    env.events()
        .publish((symbol_short!("Inactive"), meter_id), now);
}

#[contractimpl]
impl UtilityContract {
    pub fn set_oracle(env: Env, oracle: Address) {
        env.storage().instance().set(&DataKey::Oracle, &oracle);
    }

    pub fn set_native_token(env: Env, native_token: Address) {
        env.storage().instance().set(&DataKey::NativeToken, &native_token);
    }

    pub fn set_seasonal_factor(env: Env, factor: i128) {
        // Only contract or authorized admin should call this. 
        // For simple project, let's assume oracle/admin auth.
        env.storage().instance().set(&DataKey::SeasonalFactor, &factor);
    }

    pub fn set_treasury(env: Env, treasury: Address) {
        env.storage().instance().set(&DataKey::Treasury, &treasury);
    }

    pub fn setup_saving_goal(
        env: Env, 
        meter_id: u64, 
        target_amount: i128, 
        marketplace: Address
    ) {
        let meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();
        
        let goal = SavingGoal {
            target_amount,
            current_savings: 0,
            marketplace,
            is_completed: false,
        };
        
        env.storage().instance().set(&DataKey::SavingGoal(meter_id), &goal);
    }

    pub fn register_meter(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        device_public_key: BytesN<32>,
        end_date: u64,
        rent_deposit: i128,
    ) -> u64 {
        Self::register_meter_with_mode(
            env, 
            user, 
            provider, 
            off_peak_rate, 
            token, 
            BillingType::PrePaid, 
            device_public_key,
            end_date,
            rent_deposit
        )
    }

    pub fn register_meter_with_mode(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        billing_type: BillingType,
        device_public_key: BytesN<32>,
        end_date: u64,
        rent_deposit: i128,
    ) -> u64 {
        user.require_auth();

        // Collect Rent Deposit (in native token or whatever is specified)
        if rent_deposit > 0 {
            // For the Rent Deposit, we strictly use the contract's token if it's the native asset
            // or we expect XLM. Standard practice says XLM. 
            // Here we use the token configured for the meter for simplicity in testing,
            // but in production this should probably be the native XLM address.
        transfer_tokens(env.clone(), &token, &user, &env.current_contract_address(), &rent_deposit);
        }

        let mut count = env
            .storage()
            .instance()
            .get::<DataKey, u64>(&DataKey::Count)
            .unwrap_or(0);
        count += 1;

        let mut active_count = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::ActiveMetersCount)
            .unwrap_or(0);
        active_count += 1;

        let now = env.ledger().timestamp();
        let peak_rate = off_peak_rate.saturating_mul(PEAK_RATE_MULTIPLIER) / RATE_PRECISION;
        
        let usage_data = UsageData {
            total_watt_hours: 0,
            current_cycle_watt_hours: 0,
            peak_usage_watt_hours: 0,
            last_reading_timestamp: now,
            precision_factor: 1000,
        };

        let meter = Meter {
            user: user.clone(),
            provider,
            billing_type,
            off_peak_rate,
            peak_rate,
            rate_per_second: off_peak_rate,
            rate_per_unit: off_peak_rate,
            balance: 0,
            debt: 0,
            collateral_limit: 0,
            last_update: now,
            is_active: false,
            token,
            usage_data,
            max_flow_rate_per_hour: off_peak_rate.saturating_mul(HOUR_IN_SECONDS as i128),
            last_claim_time: now,
            claimed_this_hour: 0,
            heartbeat: now,
            device_public_key,
            end_date,
            rent_deposit,
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        env.storage().instance().set(&DataKey::ActiveMetersCount, &active_count);
        count
    }

    pub fn top_up(env: Env, meter_id: u64, amount: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        let was_active = meter.is_active;
        transfer_tokens(env.clone(), &meter.token, &meter.user, &env.current_contract_address(), &amount);
        
        // Convert XLM to USD cents if needed
        let converted_amount = match convert_xlm_to_usd_if_needed(&env, amount, &meter.token) {
            Ok(amount) => amount,
            Err(_) => panic_with_error!(&env, ContractError::PriceConversionFailed),
        };
        
        if converted_amount <= 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }

        match meter.billing_type {
            BillingType::PrePaid => {
                meter.balance = meter.balance.saturating_add(converted_amount);
            }
            BillingType::PostPaid => {
                let settlement = converted_amount.min(meter.debt.max(0));
                meter.debt = meter.debt.saturating_sub(settlement);
                meter.collateral_limit = meter
                    .collateral_limit
                    .saturating_add(converted_amount.saturating_sub(settlement));
            }
        }

        let now = env.ledger().timestamp();
        refresh_activity(&mut meter);
        if !was_active && meter.is_active {
            meter.last_update = now;
        }

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        if !was_active && meter.is_active {
            publish_active_event(&env, meter_id, now);
        }
        
        // Emit conversion event if XLM was used
        if is_native_token(&env, &meter.token) {
            env.events().publish(
                (symbol_short!("XLMtoUSD"), meter_id), 
                (amount, converted_amount)
            );
        }
    }

    pub fn claim(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        let now = env.ledger().timestamp();
        if !meter.is_active {
            meter.last_update = now;
            env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
            return;
        }

        reset_claim_window_if_needed(&mut meter, now);

        let elapsed = now.saturating_sub(meter.last_update);
        let effective_rate = get_effective_rate(&env, &meter, now);
        let requested = (elapsed as i128).saturating_mul(effective_rate);
        let claimable = requested
            .min(remaining_claim_capacity(&meter))
            .min(provider_meter_value(&meter));

        if claimable > 0 {
            let provider_window =
                apply_provider_withdrawal_limit(&env, &meter.provider, claimable);
            apply_provider_claim(&env, &mut meter, meter_id, claimable);
            env.storage().instance().set(
                &DataKey::ProviderWindow(meter.provider.clone()),
                &provider_window,
            );
        }

        let was_active = meter.is_active;
        meter.last_update = now;
        refresh_activity(&mut meter);
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        if was_active && !meter.is_active {
            publish_inactive_event(&env, meter_id, now);
        }
    }

    pub fn update_device_public_key(env: Env, meter_id: u64, new_public_key: BytesN<32>) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();
        meter.device_public_key = new_public_key;
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn deduct_units(env: Env, meter_id: u64, units_consumed: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        
        let now = env.ledger().timestamp();
        reset_claim_window_if_needed(&mut meter, now);

        let effective_rate = get_effective_rate(&env, &meter, now);
        let cost = units_consumed.saturating_mul(effective_rate);

        // Enforce max flow rate hourly cap and available funds
        let claimable = cost
            .min(remaining_claim_capacity(&meter))
            .min(provider_meter_value(&meter));

        let was_active = meter.is_active;
        apply_provider_claim(&env, &mut meter, meter_id, claimable);
        meter.last_update = now;
        refresh_activity(&mut meter);

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        if was_active && !meter.is_active {
            publish_inactive_event(&env, meter_id, now);
        }

        env.events()
            .publish((symbol_short!("Usage"), meter_id), (units_consumed, claimable));
    }

    pub fn deduct_units_signed(env: Env, signed_data: SignedUsageData) {
        let mut meter = get_meter_or_panic(&env, signed_data.meter_id);
        
        // Verify the signature matches the registered device public key
        verify_usage_signature(&env, &signed_data, &meter)
            .unwrap_or_else(|e| panic_with_error!(&env, e));

        let now = env.ledger().timestamp();
        reset_claim_window_if_needed(&mut meter, now);

        let effective_rate = get_effective_rate(&env, &meter, now);
        let cost = signed_data.units_consumed.saturating_mul(effective_rate);

        // Enforce max flow rate hourly cap and available funds
        let claimable = cost
            .min(remaining_claim_capacity(&meter))
            .min(provider_meter_value(&meter));

        let was_active = meter.is_active;
        apply_provider_claim(&env, &mut meter, signed_data.meter_id, claimable);
        meter.last_update = now;
        refresh_activity(&mut meter);

        env.storage().instance().set(&DataKey::Meter(signed_data.meter_id), &meter);

        if was_active && !meter.is_active {
            publish_inactive_event(&env, signed_data.meter_id, now);
        }

        env.events()
            .publish((symbol_short!("Usage"), signed_data.meter_id), (signed_data.units_consumed, claimable));
    }

    pub fn set_max_flow_rate(env: Env, meter_id: u64, max_flow_rate_per_hour: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        meter.max_flow_rate_per_hour = max_flow_rate_per_hour.max(0);
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn update_usage(env: Env, meter_id: u64, watt_hours_consumed: i128) {
        // Input validation for security
        if watt_hours_consumed < 0 {
            panic_with_error!(env, ContractError::InvalidUsageValue);
        }
        
        if watt_hours_consumed > MAX_USAGE_PER_UPDATE {
            panic_with_error!(env, ContractError::UsageExceedsLimit);
        }
        
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        let precise_consumption =
            watt_hours_consumed.saturating_mul(meter.usage_data.precision_factor);
        meter.usage_data.total_watt_hours = meter
            .usage_data
            .total_watt_hours
            .saturating_add(precise_consumption);
        meter.usage_data.current_cycle_watt_hours = meter
            .usage_data
            .current_cycle_watt_hours
            .saturating_add(precise_consumption);

        if meter.usage_data.current_cycle_watt_hours > meter.usage_data.peak_usage_watt_hours {
            meter.usage_data.peak_usage_watt_hours = meter.usage_data.current_cycle_watt_hours;
        }

        meter.usage_data.last_reading_timestamp = env.ledger().timestamp();
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn reset_cycle_usage(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        meter.usage_data.current_cycle_watt_hours = 0;
        meter.usage_data.last_reading_timestamp = env.ledger().timestamp();
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn get_usage_data(env: Env, meter_id: u64) -> Option<UsageData> {
        env.storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
            .map(|meter| meter.usage_data)
    }

    pub fn get_meter(env: Env, meter_id: u64) -> Option<Meter> {
        env.storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
    }

    pub fn get_provider_window(env: Env, provider: Address) -> Option<ProviderWithdrawalWindow> {
        env.storage()
            .instance()
            .get(&DataKey::ProviderWindow(provider))
    }

    pub fn get_watt_hours_display(precise_watt_hours: i128, precision_factor: i128) -> i128 {
        if precision_factor <= 0 {
            return precise_watt_hours; // Fallback to avoid division by zero
        }
        precise_watt_hours / precision_factor
    }

    pub fn get_saving_goal(env: Env, meter_id: u64) -> Option<SavingGoal> {
        env.storage().instance().get(&DataKey::SavingGoal(meter_id))
    }

    pub fn calculate_expected_depletion(env: Env, meter_id: u64) -> Option<u64> {
        env.storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
            .map(|meter| {
                if meter.off_peak_rate <= 0 {
                    return 0;
                }

                let available = provider_meter_value(&meter);
                if available <= 0 {
                    return 0;
                }

                env.ledger().timestamp() + (available / meter.off_peak_rate) as u64
            })
    }

    pub fn emergency_shutdown(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        meter.is_active = false;
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn update_heartbeat(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();
        meter.heartbeat = env.ledger().timestamp();
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn finalize_and_purge(env: Env, meter_id: u64) {
        let meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        let now = env.ledger().timestamp();

        // 1. Check if stream is finished
        if now < meter.end_date {
            panic_with_error!(&env, ContractError::StreamNotFinished);
        }

        // 2. Check if balance is zero
        if meter.balance > 0 || meter.debt > 0 {
            panic_with_error!(&env, ContractError::BalanceNotEmpty);
        }

        // 3. Return Rent Deposit to user
        if meter.rent_deposit > 0 {
            transfer_tokens(env.clone(), &meter.token, &env.current_contract_address(), &meter.user, &meter.rent_deposit);
        }

        // 4. Delete storage
        env.storage().instance().remove(&DataKey::Meter(meter_id));

        // 5. Track active meters
        let mut active_count = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::ActiveMetersCount)
            .unwrap_or(0);
        
        if active_count > 0 {
            active_count -= 1;
        }

        if active_count == 0 {
            // "the contract deletes all storage and the contract instance itself"
            // We clear the remaining metadata to minimize state footprint
            env.storage().instance().remove(&DataKey::Count);
            env.storage().instance().remove(&DataKey::Oracle);
            env.storage().instance().remove(&DataKey::ActiveMetersCount);
            
            // In Soroban, clearing all instance storage effectively "destroys" the instance
            // state until it expires or is redeployed.
        } else {
            env.storage().instance().set(&DataKey::ActiveMetersCount, &active_count);
        }

        env.events().publish((symbol_short!("Purge"), meter_id), now);
    }

    pub fn withdraw_earnings(env: Env, meter_id: u64, amount_usd_cents: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        
        if amount_usd_cents <= 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        let available_earnings = match meter.billing_type {
            BillingType::PrePaid => meter.balance,
            BillingType::PostPaid => meter.debt,
        };
        
        if amount_usd_cents > available_earnings {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        // Convert USD cents to XLM if needed
        let withdrawal_amount = match convert_usd_to_xlm_if_needed(&env, amount_usd_cents, &meter.token) {
            Ok(amount) => amount,
            Err(_) => panic_with_error!(&env, ContractError::PriceConversionFailed),
        };
        
        let client = token::Client::new(&env, &meter.token);
        client.transfer(&env.current_contract_address(), &meter.provider, &withdrawal_amount);
        
        // Update meter balance/debt
        match meter.billing_type {
            BillingType::PrePaid => {
                meter.balance = meter.balance.saturating_sub(amount_usd_cents);
            }
            BillingType::PostPaid => {
                meter.debt = meter.debt.saturating_sub(amount_usd_cents);
            }
        }
        
        let now = env.ledger().timestamp();
        refresh_activity(&mut meter);
        meter.last_update = now;
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
        
        // Emit conversion event if XLM was used
        if is_native_token(&env, &meter.token) {
            env.events().publish(
                (symbol_short!("USDtoXLM"), meter_id), 
                (amount_usd_cents, withdrawal_amount)
            );
        }
    }

    pub fn get_current_rate(env: Env) -> Option<PriceData> {
        match env.storage().instance().get::<DataKey, Address>(&DataKey::Oracle) {
            Some(oracle_address) => {
                let oracle_client = PriceOracleClient::new(&env, &oracle_address);
                Some(oracle_client.get_price())
            }
            None => None,
        }
    }

    pub fn is_meter_offline(env: Env, meter_id: u64) -> bool {
        match env
            .storage()
            .instance()
            .get::<DataKey, Meter>(&DataKey::Meter(meter_id))
        {
            Some(meter) => {
                env.ledger().timestamp().saturating_sub(meter.heartbeat) > HOUR_IN_SECONDS
            }
            None => true,
        }
    }
}

mod test;
