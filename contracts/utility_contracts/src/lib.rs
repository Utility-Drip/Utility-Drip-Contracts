#![no_std]
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    Address, Env, BytesN, Vec, Symbol,
};

// --- Constants (Merged) ---
const DEFAULT_BUFFER_DAYS: i128 = 3;
const TRUSTED_BUFFER_DAYS: i128 = 1;
const MINIMUM_BALANCE_TO_FLOW: i128 = 500;
const HOUR_IN_SECONDS: u64 = 60 * 60;
const DAY_IN_SECONDS: u64 = 24 * HOUR_IN_SECONDS;
const GRACE_PERIOD_SECONDS: u64 = 86_400;
const DEBT_THRESHOLD: i128 = -10_000_000;
const MAX_USAGE_PER_UPDATE: i128 = 1_000_000_000_000i128;
const MAX_TIMESTAMP_DELAY: u64 = 300;
const PEAK_HOUR_START: u64 = 18 * HOUR_IN_SECONDS;
const PEAK_HOUR_END: u64 = 21 * HOUR_IN_SECONDS;
const PEAK_RATE_MULTIPLIER: i128 = 3; 
const RATE_PRECISION: i128 = 2;
const XLM_PRECISION: i128 = 10_000_000;
const DEFAULT_TAX_RATE_BPS: i128 = 500;
const MAINTENANCE_FUND_PERCENT_BPS: i128 = 1;
const LEDGER_LIFETIME_EXTENSION: u32 = 1_000_000;
const AUTO_EXTEND_LEDGER_THRESHOLD: u32 = 500_000;
const UPGRADE_VETO_PERIOD_SECONDS: u64 = 7 * DAY_IN_SECONDS;
const ADMIN_TRANSFER_TIMELOCK: u64 = 48 * HOUR_IN_SECONDS;
const VETO_THRESHOLD_BPS: i128 = 1000;
const WITHDRAWAL_REQUEST_EXPIRY: u64 = 7 * DAY_IN_SECONDS;
const MIN_FINANCE_WALLETS: usize = 3;
const MAX_FINANCE_WALLETS: usize = 5;

// --- Data Structures ---

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BillingType {
    PrePaid,
    PostPaid,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StreamStatus {
    Active = 0,
    Paused = 1,
    Depleted = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuousFlow {
    // Tightly packed struct for optimal storage
    pub stream_id: u64,           // 8 bytes
    pub flow_rate_per_second: i128, // 16 bytes - micro-stroops per second
    pub accumulated_balance: i128,  // 16 bytes - precise balance tracking
    pub last_flow_timestamp: u64,   // 8 bytes - u64 for epoch safety
    pub created_timestamp: u64,     // 8 bytes - creation time
    pub status: StreamStatus,       // 1 byte (enum)
    pub reserved: [u8; 7],         // 7 bytes - for future use/alignment
}
// Minimum balance required to keep the IoT relay open (500 tokens for testing)
const MINIMUM_BALANCE_TO_FLOW: i128 = 500; // 500 tokens minimum for testing

#[contracttype]
#[derive(Clone)]
pub struct UsageReport {
    pub meter_id: u64,
    pub timestamp: u64,
    pub watt_hours_consumed: i128,
    pub units_consumed: i128,
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
    pub renewable_watt_hours: i128,
    pub renewable_percentage: i128,
    pub monthly_volume: i128,
    pub last_volume_reset: u64,
}

mod gas_estimator;
use gas_estimator::GasCostEstimator;

pub mod grant_stream_listener;
pub mod velocity_limit;
use velocity_limit::{check_velocity_limits, apply_override, revoke_override, get_velocity_config, set_velocity_config, VelocityDataKey};
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavingGoal {
    pub target_amount: i128,
    pub current_savings: i128,
    pub marketplace: Address,
    pub is_completed: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct Meter {
    pub user: Address,
    pub provider: Address,
    pub billing_type: BillingType,
    pub off_peak_rate: i128,
    pub peak_rate: i128,
    pub rate_per_unit: i128,
    pub balance: i128,
    pub debt: i128,
    pub last_update: u64,
    pub is_active: bool,
    pub token: Address,
    pub usage_data: UsageData,
    pub device_public_key: BytesN<32>,
    pub end_date: u64,
    pub rent_deposit: i128,
    pub priority_index: u32,
    pub green_energy_discount_bps: i128,
    pub is_paused: bool,
    pub is_disputed: bool,
    pub challenge_timestamp: u64,
    pub credit_drip_rate: i128,
    pub is_closed: bool,
    pub priority_index: u32,
    pub off_peak_reward_rate_bps: i128,
    pub milestone_deadline: u64,
    pub milestone_confirmed: bool,
}

#[contracttype]
#[derive(Clone)]
pub struct ClaimSettlement {
    pub gross_claimed: i128,
    pub provider_payout: i128,
    pub tax_amount: i128,
    pub protocol_fee: i128,
    pub reseller_payout: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct ResellerConfig {
    pub reseller: Address,
    pub fee_bps: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct ImpactMetrics {
    pub total_kilowatts_funded: i128,
    pub total_liters_streamed: i128,
    pub active_meters: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct ConservationGoal {
    pub goal_id: u64,
    pub provider: Address,
    pub target_water_savings: i128,  // in liters
    pub current_savings: i128,
    pub deadline: u64,
    pub is_active: bool,
    pub grant_amount: i128,  // grant amount when goal is reached
    pub grant_token: Address,
    pub created_at: u64,
    pub achieved_at: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
pub struct GoalReachedEvent {
    pub goal_id: u64,
    pub provider: Address,
    pub water_savings: i128,
    pub grant_amount: i128,
    pub grant_token: Address,
    pub achieved_at: u64,
}

#[contractclient(name = "GrantStreamClient")]
pub trait GrantStream {
    fn on_goal_reached(env: Env, goal_event: GoalReachedEvent);
}

// Issue #118: Zero-Knowledge Privacy Usage Reporting
// ZK-proof structures for private billing and usage verification
#[contracttype]
#[derive(Clone)]
pub struct ZKProof {
    pub commitment: BytesN<32>,        // Pedersen commitment to usage amount
    pub nullifier: BytesN<32>,         // Nullifier to prevent double-spending
    pub proof: Bytes,                  // ZK-SNARK proof (placeholder for future implementation)
    pub meter_id: u64,                 // Associated meter ID
    pub timestamp: u64,                // Proof generation timestamp
    pub is_valid: bool,                // Proof validity status
}

#[contracttype]
#[derive(Clone)]
pub struct ZKUsageReport {
    pub commitment: BytesN<32>,        // Commitment to usage data
    pub nullifier: BytesN<32>,         // Unique nullifier for this report
    pub encrypted_usage: Bytes,         // Encrypted usage data (for future ZK implementation)
    pub proof_hash: BytesN<32>,        // Hash of the ZK proof
    pub meter_id: u64,                 // Meter identifier
    pub billing_cycle: u32,             // Billing cycle number
    pub timestamp: u64,                // Report timestamp
    pub is_verified: bool,              // Verification status
}

#[contracttype]
#[derive(Clone)]
pub struct PrivateBillingStatus {
    pub meter_id: u64,                 // Meter ID
    pub billing_cycle: u32,            // Current billing cycle
    pub total_commitments: u32,        // Number of commitments received
    pub verified_proofs: u32,          // Number of verified ZK proofs
    pub last_verification: u64,        // Last verification timestamp
    pub privacy_enabled: bool,         // Whether privacy mode is enabled
}

#[contracttype]
#[derive(Clone)]
pub struct CommitmentBatch {
    pub commitments: Vec<BytesN<32>>,  // Batch of commitments
    pub nullifiers: Vec<BytesN<32>>,   // Corresponding nullifiers
    pub batch_root: BytesN<32>,       // Merkle root of commitments
    pub timestamp: u64,                // Batch creation time
    pub meter_id: u64,                 // Associated meter
}

#[contracttype]
#[derive(Clone)]
pub struct MeterStatus {
    pub meter_id: u64,
    pub is_active: bool,
    pub balance: i128,
    pub billing_cycle: u32,
    pub total_commitments: u32,
    pub verified_proofs: u32,
    pub privacy_enabled: bool,
    pub last_update: u64,
    pub usage_summary: Option<UsageData>,
}

// Issue #98: Multi-Sig Provider Withdrawal Requirement
// For large utility companies, withdrawals require 3-of-5 authorized signatures
// from Finance Department wallets to prevent unauthorized access to streaming revenue
#[contracttype]
#[derive(Clone)]
pub struct MultiSigConfig {
    pub provider: Address,              // The utility provider this config belongs to
    pub finance_wallets: Vec<Address>,  // List of authorized Finance Department wallets (max 5)
    pub required_signatures: u32,       // Number of signatures required (typically 3)
    pub threshold_amount: i128,         // Minimum amount requiring multi-sig (in USD cents)
    pub is_active: bool,                // Whether multi-sig is enabled
    pub created_at: u64,                // Timestamp when config was created
}

#[contracttype]
#[derive(Clone)]
pub struct WithdrawalRequest {
    pub request_id: u64,                // Unique request identifier
    pub provider: Address,              // Provider requesting withdrawal
    pub meter_id: u64,                  // Meter to withdraw from
    pub amount_usd_cents: i128,         // Amount requested in USD cents
    pub destination: Address,           // Destination treasury address
    pub proposer: Address,              // Finance wallet that proposed this request
    pub created_at: u64,                // Timestamp when request was created
    pub expires_at: u64,                // Request expiration timestamp
    pub approval_count: u32,            // Current number of approvals
    pub is_executed: bool,              // Whether withdrawal has been executed
    pub is_cancelled: bool,             // Whether request was cancelled
}

#[contracttype]
#[derive(Clone)]
pub struct StreamUpdatedEvent {
    pub stream_id: u64,
    pub old_flow_rate: i128,
    pub new_flow_rate: i128,
    pub timestamp: u64,
    pub old_status: StreamStatus,
    pub new_status: StreamStatus,
}

#[contracttype]
pub enum DataKey {
    Meter(u64),
    Count,
    Oracle,
    ActiveMetersCount,
    SeasonalFactor,
    Treasury,
    ProviderVolume(Address),
    SavingGoal(u64),
    NativeToken,
    TaxRateBps,
    ProtocolFeeBps,
    SupportedToken(Address),
    SupportedWithdrawalToken(Address),
    ProviderTotalPool(Address),
    ContinuousFlow(u64),
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
    PairingAlreadyComplete = 12,
    ChallengeNotFound = 13,
    InvalidPairingSignature = 14,
    MeterNotPaired = 15,
    MeterPaused = 16,
    AlreadyVoted = 17,
    InvalidClosingFee = 18,
    AccountAlreadyClosed = 19,
    InsufficientBalance = 20,
    UnauthorizedContributor = 21,
    InDispute = 22,
    ChallengeActive = 23,
    NotAnOracle = 24,
    // Task #1: Priority System Errors
    ThrottlingThresholdExceeded = 25,
    LowPriorityStreamPaused = 26,
    // Task #2: Tax Compliance Errors
    GovernmentVaultNotSet = 27,
    TaxCalculationFailed = 28,
    // Task #3: Maintenance Errors
    MaintenanceFundInsufficient = 29,
    TTLExtensionFailed = 30,
    // Task #4: Upgrade Errors
    UpgradeProposalActive = 31,
    VetoPeriodExpired = 32,
    UserVetoedProposal = 33,
    InvalidWasmHash = 34,
    // NEW TASKS:
    // Task #1: Admin Transfer Errors
    AdminTransferActive = 35,
    NoAdminTransferInProgress = 36,
    VetoThresholdNotReached = 37,
    AdminExecutionWindowExpired = 38,
    NotCurrentAdmin = 39,
    // Task #2: Legal Freeze Errors
    NotComplianceOfficer = 40,
    MeterNotFrozen = 41,
    LegalFreezeAlreadyActive = 42,
    ComplianceCouncilApprovalRequired = 43,
    // Task #3: Verified Provider Errors
    ProviderNotVerified = 44,
    VerificationAlreadyGranted = 45,
    // Task #4: Sub-DAO Errors
    NotParentDao = 46,
    SubDaoBudgetExceeded = 47,
    SubDaoNotConfigured = 48,
    // Issue #98: Multi-Sig Withdrawal Errors
    MultiSigNotConfigured = 49,
    MultiSigAlreadyConfigured = 50,
    InvalidFinanceWalletCount = 51,
    InvalidSignatureThreshold = 52,
    NotAuthorizedFinanceWallet = 53,
    WithdrawalRequestNotFound = 54,
    WithdrawalRequestExpired = 55,
    WithdrawalAlreadyExecuted = 56,
    WithdrawalAlreadyCancelled = 57,
    InsufficientApprovals = 58,
    AlreadyApprovedWithdrawal = 59,
    NotApprovedByWallet = 60,
    AmountBelowMultiSigThreshold = 61,
    MultiSigRequiredForAmount = 62,
    // Issue #118: ZK Privacy Errors
    InvalidCommitment = 63,
    NullifierAlreadyUsed = 64,
    InvalidZKProof = 65,
    PrivacyNotEnabled = 66,
    CommitmentNotFound = 67,
    InvalidBillingCycle = 68,
    ZKVerificationFailed = 69,
    // Issue #130: Grant Stream Integration Errors
    ConservationGoalNotFound = 70,
    GoalAlreadyAchieved = 71,
    GoalExpired = 72,
    InvalidGrantAmount = 73,
    GrantStreamNotConfigured = 74,
    InsufficientWaterSavings = 75,
    // Streaming-Limit Circuit Breaker Errors
    PerStreamVelocityLimitExceeded = 76,
    GlobalVelocityLimitExceeded = 77,
    VelocityLimitBreach = 78,
}

#[contracttype]
#[derive(Clone)]
pub struct PairingChallengeData {
    pub contract: Address,
    pub meter_id: u64,
    pub timestamp: u64,
}

// --- Internal Helpers ---

fn get_seasonal_multiplier(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::SeasonalFactor).unwrap_or(100)
}

fn is_peak_hour(timestamp: u64) -> bool {
    let day_seconds = timestamp % DAY_IN_SECONDS;
    day_seconds >= PEAK_HOUR_START && day_seconds <= PEAK_HOUR_END
}

fn get_effective_rate(env: &Env, meter: &Meter, timestamp: u64) -> i128 {
    let base_rate = if is_peak_hour(timestamp) {
        meter.peak_rate
    } else {
        ((raw_usd - 50) / 100) * 100 // Round down on -.5 or lower
    }
}

    pub fn claim(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        if meter.is_disputed { panic_with_error!(&env, ContractError::InDispute); }

        let old_meter_value = provider_meter_value(&meter);
        let now = env.ledger().timestamp();
        let mut window = get_provider_window_or_default(&env, &meter.provider, now);
        
        let settlement = settle_claim_for_meter(&env, meter_id, &mut meter, now, &mut window);
        
        // === STREAMING-LIMIT CIRCUIT BREAKER ===
        // Check velocity limits BEFORE processing the claim
        // This prevents smart contract bugs from draining streams too quickly
        if settlement.gross_claimed > 0 {
            // Check per-stream velocity limit
            if let Err(error) = check_velocity_limits(&env, meter_id, &meter.provider, settlement.gross_claimed) {
                if error == symbol_short!("vlimit") {
                    panic_with_error!(&env, ContractError::PerStreamVelocityLimitExceeded);
                } else if error == symbol_short!("gvlimit") {
                    panic_with_error!(&env, ContractError::GlobalVelocityLimitExceeded);
                } else {
                    panic_with_error!(&env, ContractError::VelocityLimitBreach);
                }
            }
        }
        
        let client = token::Client::new(&env, &meter.token);

        // 1. Pay Government Tax
        if settlement.tax_amount > 0 {
            if let Some(gov_vault) = env.storage().instance().get::<_, Address>(&DataKey::GovernmentVault) {
                client.transfer(&env.current_contract_address(), &gov_vault, &settlement.tax_amount);
            }
        }

        // 2. Pay Protocol Maintenance Fee
        if settlement.protocol_fee > 0 {
            if let Some(wallet) = env.storage().instance().get::<_, Address>(&DataKey::MaintenanceWallet) {
                client.transfer(&env.current_contract_address(), &wallet, &settlement.protocol_fee);
            }
        }

        // 3. Pay Reseller (if applicable)
        if settlement.reseller_payout > 0 {
            if let Some(reseller_config) = get_reseller_config_impl(&env, meter_id) {
                client.transfer(&env.current_contract_address(), &reseller_config.reseller, &settlement.reseller_payout);
            }
        }

        // 4. Pay Provider
        if settlement.provider_payout > 0 {
            client.transfer(&env.current_contract_address(), &meter.provider, &settlement.provider_payout);
        }

        // Update State
        env.storage().instance().set(&DataKey::ProviderWindow(meter.provider.clone()), &window);
        update_provider_total_pool(&env, &meter.provider, old_meter_value, provider_meter_value(&meter));
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

fn convert_usd_to_token_if_needed(env: &Env, usd_cents: i128, destination_token: &Address) -> Result<i128, ContractError> {
    // For now, we assume the oracle can provide conversion rates for any token
    // In a real implementation, you'd need specific price feeds for each token
    match env.storage().instance().get::<DataKey, Address>(&DataKey::Oracle) {
        Some(oracle_address) => {
            let oracle_client = PriceOracleClient::new(env, &oracle_address);
            let price_data = oracle_client.get_price();

    let mut provider_share = amount;
    
    // 1. Savings Goal Redirection (20%)
    if let Some(mut goal) = env.storage().instance().get::<DataKey, SavingGoal>(&DataKey::SavingGoal(meter_id)) {
        if !goal.is_completed {
            let contribution = amount / 5; 
            goal.current_savings = goal.current_savings.saturating_add(contribution);
            provider_share = provider_share.saturating_sub(contribution);

            if goal.current_savings >= goal.target_amount {
                goal.is_completed = true;
                env.events().publish((symbol_short!("AutoBuy"), meter.user.clone()), (goal.marketplace.clone(), goal.target_amount));
            }
        }
        None => Err(ContractError::OracleNotSet),
    }
}

    pub fn assign_reseller(env: Env, meter_id: u64, reseller: Address, fee_bps: i128) {
        let meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        if fee_bps > MAX_RESELLER_FEE_BPS { panic_with_error!(&env, ContractError::InvalidResellerFee); }

        let config = ResellerConfig { reseller: reseller.clone(), fee_bps };
        env.storage().instance().set(&DataKey::ResellerConfig(meter_id), &config);
        env.events().publish((symbol_short!("RslrSet"), meter_id), (reseller, fee_bps));
    }

    pub fn claim_impact_sbt(env: Env, meter_id: u64) {
        let meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        if env.storage().instance().get(&DataKey::ImpactSBTMinted(meter_id)).unwrap_or(false) {
            panic_with_error!(&env, ContractError::SBTAlreadyMinted);
        }

        const SBT_THRESHOLD: i128 = 18_250_000;
        if meter.usage_data.renewable_watt_hours < SBT_THRESHOLD {
            panic_with_error!(&env, ContractError::ImpactNotSignificantEnough);
        }

        let carbon_saved = meter.usage_data.renewable_watt_hours.saturating_mul(4) / 10;
        let susu_addr = env.storage().instance().get::<_, Address>(&DataKey::SoroSusuContract).expect("No Susu");
        let susu_client = SoroSusuClient::new(&env, &susu_addr);
        let score = susu_client.get_susu_score(meter.user.clone());

        if let Some(minter_addr) = env.storage().instance().get::<_, Address>(&DataKey::NFTMinter) {
            let minter = NFTMinterClient::new(&env, &minter_addr);
            minter.mint_impact_sbt(&meter.user, &carbon_saved, &score);
            env.storage().instance().set(&DataKey::ImpactSBTMinted(meter_id), &true);
        }
    }

    // 2. Sustainability / Protocol Fee (0.1% if vol > $100k)
    let mut provider_vol = env.storage().instance().get::<DataKey, i128>(&DataKey::ProviderVolume(meter.provider.clone())).unwrap_or(0);
    if provider_vol >= 10_000_000 {
        let fee = provider_share / 1000;
        if fee > 0 {
            if let Some(treasury) = env.storage().instance().get::<DataKey, Address>(&DataKey::Treasury) {
                let client = token::Client::new(env, &meter.token);
                client.transfer(&env.current_contract_address(), &treasury, &fee);
                provider_share = provider_share.saturating_sub(fee);
            }
        }
        ImpactMetrics { total_kilowatts_funded: total_wh / 1000, total_liters_streamed: total_val, active_meters: active }
    }

// --- Internal Settlement Logic ---

fn settle_claim_for_meter(
    env: &Env,
    meter_id: u64,
    meter: &mut Meter,
    now: u64,
    provider_window: &mut ProviderWithdrawalWindow,
) -> ClaimSettlement {
    let elapsed = now.saturating_sub(meter.last_update);
    let mut amount = (elapsed as i128).saturating_mul(meter.rate_per_unit);
    
    // Issue #106: Milestone Penalty (Halve rate if deadline missed)
    if meter.milestone_deadline > 0 && now > meter.milestone_deadline && !meter.milestone_confirmed {
        amount /= 2;
    }

    let claimable = if amount > meter.balance && meter.balance - amount >= DEBT_THRESHOLD {
        amount
    } else if amount > meter.balance {
        meter.balance - DEBT_THRESHOLD
    } else {
        amount
    };

    if claimable <= 0 {
        return ClaimSettlement { gross_claimed: 0, provider_payout: 0, tax_amount: 0, protocol_fee: 0, reseller_payout: 0 };
    }

    // 1. Tax Calculation
    let tax_rate = env.storage().instance().get(&DataKey::TaxRateBps).unwrap_or(DEFAULT_TAX_RATE_BPS);
    let tax_amt = (claimable * tax_rate) / 10000;
    let after_tax = claimable - tax_amt;

    // 2. Protocol Fee
    let protocol_bps: i128 = env.storage().instance().get(&DataKey::ProtocolFeeBps).unwrap_or(0);
    let protocol_fee = (after_tax * protocol_bps) / 10000;
    let after_protocol = after_tax - protocol_fee;

    // 3. Reseller Split
    let reseller_payout = get_reseller_cut(env, meter_id, after_protocol);
    let provider_payout = after_protocol - reseller_payout;

    meter.balance -= claimable;
    meter.last_update = now;

    ClaimSettlement {
        gross_claimed: claimable,
        provider_payout,
        tax_amount: tax_amt,
        protocol_fee,
        reseller_payout,
    };

    // Calculate total value (balance + debt if postpaid)
    let total_value = match meter.billing_type {
        BillingType::PrePaid => meter.balance,
        BillingType::PostPaid => meter.balance.saturating_sub(meter.debt),
    };

    if total_value <= 0 {
        return false;
    }

    // If balance is less than 20% of total value, trigger throttling
    let threshold = (total_value * THROTTLING_THRESHOLD_PERCENT) / 100;
    meter.balance < threshold
}

fn should_pause_low_priority_stream(meter: &Meter, throttling_active: bool) -> bool {
    // Only pause if throttling is active AND this is a low priority stream
    throttling_active && meter.priority_index >= LOW_PRIORITY_THRESHOLD
}

// --- Helpers ---

fn get_meter_or_panic(env: &Env, id: u64) -> Meter {
    env.storage().instance().get(&DataKey::Meter(id)).expect("Meter Not Found")
}

fn provider_meter_value(meter: &Meter) -> i128 {
    meter.balance.max(DEBT_THRESHOLD)
}

// Task #3: Self-Maintenance Helper Functions
fn allocate_to_maintenance_fund(env: &Env, meter_id: u64, amount: i128) {
    let maintenance_amount = (amount * MAINTENANCE_FUND_PERCENT_BPS) / 10_000;

    if maintenance_amount > 0 {
        let current_fund: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MaintenanceFund(meter_id))
            .unwrap_or(0);

        let new_fund = current_fund.saturating_add(maintenance_amount);
        env.storage()
            .instance()
            .set(&DataKey::MaintenanceFund(meter_id), &new_fund);
    }
}

fn get_reseller_config_impl(env: &Env, meter_id: u64) -> Option<ResellerConfig> {
    env.storage().instance().get(&DataKey::ResellerConfig(meter_id))
}

fn auto_extend_ttl_if_needed(env: &Env, meter_id: u64) {
    let ledger_sequence = env.ledger().sequence();
    let threshold: u32 = env
        .storage()
        .instance()
        .get(&DataKey::AutoExtendThreshold)
        .unwrap_or(AUTO_EXTEND_LEDGER_THRESHOLD);

    // Check if we need to extend (every 500,000 ledgers)
    if ledger_sequence % threshold as u32 == 0 {
        let maintenance_balance = get_maintenance_fund_balance(env, meter_id);

        // Estimate cost of TTL extension (simplified - actual cost depends on storage size)
        let estimated_cost = 1_000_000; // 1 XLM in stroops as example

        if maintenance_balance >= estimated_cost {
            // Deduct from maintenance fund
            let new_balance = maintenance_balance.saturating_sub(estimated_cost);
            env.storage()
                .instance()
                .set(&DataKey::MaintenanceFund(meter_id), &new_balance);

            // Extend TTL - this extends the contract's storage TTL
            env.storage().instance().extend_ttl(LEDGER_LIFETIME_EXTENSION, LEDGER_LIFETIME_EXTENSION);

            env.events().publish(
                (soroban_sdk::symbol_short!("TTLExtnd"), meter_id),
                (ledger_sequence, LEDGER_LIFETIME_EXTENSION),
            );
        }
    }
}

// Task #4: Wasm Hash Rotation Helper Functions
fn propose_upgrade_impl(env: &Env, new_wasm_hash: BytesN<32>, proposer: &Address) -> u64 {
    let now = env.ledger().timestamp();
    let veto_deadline = now.saturating_add(UPGRADE_VETO_PERIOD_SECONDS);

    let proposal = UpgradeProposal {
        new_wasm_hash: new_wasm_hash.clone(),
        proposed_at: now,
        veto_deadline,
        proposer: proposer.clone(),
    };

    env.storage().instance().set(&DataKey::ProposedUpgrade, &proposal);
    env.storage().instance().set(&DataKey::UpgradeProposalTime, &now);
    env.storage().instance().set(&DataKey::VetoDeadline, &veto_deadline);

    env.events().publish(
        (soroban_sdk::symbol_short!("UpgrdPrp"),),
        (new_wasm_hash, now, veto_deadline),
    );

    now // Return proposal ID (using timestamp as simple ID)
}

fn has_user_vetoed(env: &Env, user: &Address, proposal_id: u64) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::UserVetoed(user.clone(), proposal_id))
        .unwrap_or(false)
}

fn submit_veto(env: &Env, user: &Address, proposal_id: u64) {
    env.storage()
        .instance()
        .set(&DataKey::UserVetoed(user.clone(), proposal_id), &true);

    env.events().publish(
        soroban_sdk::symbol_short!("VetoSubmt"),
        (user, proposal_id),
    );
}

fn can_finalize_upgrade(env: &Env) -> bool {
    // Check if veto period has expired
    let deadline: u64 = env.storage().instance().get(&DataKey::VetoDeadline).unwrap_or(0);
    let now = env.ledger().timestamp();

    match meter.billing_type {
        BillingType::PrePaid => meter.balance = meter.balance.saturating_sub(amount),
        BillingType::PostPaid => meter.debt = meter.debt.saturating_add(amount),
    }
}

#[contract]
pub struct UtilityContract;

// Issue #118: ZK Privacy Helper Functions

/// Placeholder ZK proof verification (for future full ZK-SNARK implementation)
/// This is a simple mock verification that checks basic constraints
fn verify_zk_proof_placeholder(env: &Env, proof_hash: BytesN<32>) -> bool {
    let now = env.ledger().timestamp();
    
    // Simple validation rules for placeholder implementation:
    // 1. Proof hash should not be all zeros
    // 2. Basic timestamp validation (proof should be recent)
    // 3. In production, this would be full ZK-SNARK verification
    
    let mut is_non_zero = false;
    for byte in proof_hash.to_array().iter() {
        if *byte != 0 {
            is_non_zero = true;
            break;
        }
    }
    
    // For now, accept any non-zero hash as valid (placeholder logic)
    // In production, this would involve cryptographic verification
    is_non_zero
}

/// Generate a simple commitment hash (placeholder for Pedersen commitment)
fn generate_commitment_placeholder(env: &Env, usage_amount: i128, randomness: BytesN<32>) -> BytesN<32> {
    // This is a placeholder - in production would use Pedersen commitments
    let mut combined = Vec::new(&env);
    combined.push_back(&Bytes::from_slice(&env, &usage_amount.to_be_bytes()));
    combined.push_back(&randomness);
    
    // Simple hash (placeholder - would use proper cryptographic commitment in production)
    env.crypto().sha256(&combined.to_xdr(&env))
}

/// Check if a nullifier has been used before
fn is_nullifier_used(env: &Env, nullifier: BytesN<32>) -> bool {
    env.storage().instance().has(&DataKey::NullifierMap(nullifier))
}

/// Store nullifier to prevent double-spending
fn store_nullifier(env: &Env, nullifier: BytesN<32>) {
    env.storage().instance().set(&DataKey::NullifierMap(nullifier), &true);
}

// Continuous Flow Math Engine Functions

/// Create a new continuous flow stream with timestamp-based tracking
fn create_continuous_flow(
    stream_id: u64,
    flow_rate_per_second: i128,
    initial_balance: i128,
    current_timestamp: u64,
) -> ContinuousFlow {
    ContinuousFlow {
        stream_id,
        flow_rate_per_second,
        accumulated_balance: initial_balance,
        last_flow_timestamp: current_timestamp,
        created_timestamp: current_timestamp,
        status: if initial_balance > 0 { StreamStatus::Active } else { StreamStatus::Paused },
        reserved: [0u8; 7],
    }
}

/// Calculate flow accumulation since last update with precise timestamp math
fn calculate_flow_accumulation(
    flow: &ContinuousFlow,
    current_timestamp: u64,
) -> i128 {
    if flow.status != StreamStatus::Active {
        return 0;
    }

    // Prevent underflow with checked subtraction
    let elapsed_seconds = match current_timestamp.checked_sub(flow.last_flow_timestamp) {
        Some(elapsed) => elapsed,
        None => return 0, // Timestamp went backwards, no accumulation
    };

    // Use i128 for precise calculation to prevent overflow
    let elapsed_i128 = elapsed_seconds as i128;
    
    // Calculate accumulated flow: rate * time
    // flow_rate_per_second is in micro-stroops per second
    let accumulated = flow.flow_rate_per_second.saturating_mul(elapsed_i128);
    
    accumulated
}

/// Update flow with new timestamp and handle underflow risks
fn update_continuous_flow(
    flow: &mut ContinuousFlow,
    current_timestamp: u64,
) -> Result<i128, ContractError> {
    let accumulation = calculate_flow_accumulation(flow, current_timestamp);
    
    // Handle underflow: ensure we don't go below zero balance
    if flow.accumulated_balance < accumulation {
        // Deplete the balance and set status to Depleted
        let actual_deduction = flow.accumulated_balance;
        flow.accumulated_balance = 0;
        flow.status = StreamStatus::Depleted;
        flow.last_flow_timestamp = current_timestamp;
        return Ok(actual_deduction);
    }
    
    // Normal case: deduct accumulation from balance
    flow.accumulated_balance = flow.accumulated_balance.saturating_sub(accumulation);
    flow.last_flow_timestamp = current_timestamp;
    
    // Update status based on remaining balance
    if flow.accumulated_balance == 0 {
        flow.status = StreamStatus::Depleted;
    } else if flow.status == StreamStatus::Paused && flow.accumulated_balance > 0 {
        flow.status = StreamStatus::Active;
    }
    
    Ok(accumulation)
}

/// Update flow rate with authentication and event emission
fn update_flow_rate(
    env: &Env,
    stream_id: u64,
    new_flow_rate: i128,
) -> Result<(), ContractError> {
    let mut flow = get_continuous_flow_or_panic(env, stream_id);
    
    // Require authentication for flow rate changes
    env.current_contract_address().require_auth();
    
    let old_flow_rate = flow.flow_rate_per_second;
    let old_status = flow.status;
    
    flow.flow_rate_per_second = new_flow_rate;
    
    // Update status based on new flow rate and balance
    if new_flow_rate == 0 {
        flow.status = StreamStatus::Paused;
    } else if flow.accumulated_balance > 0 && flow.status == StreamStatus::Paused {
        flow.status = StreamStatus::Active;
    }
    
    // Update timestamp to current time
    let current_timestamp = env.ledger().timestamp();
    
    // Emit detailed StreamUpdated event
    let event = StreamUpdatedEvent {
        stream_id,
        old_flow_rate,
        new_flow_rate,
        timestamp: current_timestamp,
        old_status,
        new_status: flow.status,
    };
    
    env.events().publish(
        symbol_short!("StreamUpdated"),
        (stream_id, old_flow_rate, new_flow_rate, current_timestamp, old_status as u32, flow.status as u32)
    );
    
    // Store updated flow
    env.storage()
        .instance()
        .set(&DataKey::ContinuousFlow(stream_id), &flow);
    
    Ok(())
}

/// Get continuous flow or panic if not found
fn get_continuous_flow_or_panic(env: &Env, stream_id: u64) -> ContinuousFlow {
    match env
        .storage()
        .instance()
        .get::<DataKey, ContinuousFlow>(&DataKey::ContinuousFlow(stream_id))
    {
        Some(flow) => flow,
        None => panic_with_error!(env, ContractError::MeterNotFound), // Reuse existing error
    }
}

/// Add balance to continuous flow with underflow protection
fn add_balance_to_flow(
    env: &Env,
    stream_id: u64,
    additional_balance: i128,
) -> Result<(), ContractError> {
    if additional_balance <= 0 {
        return Err(ContractError::InvalidTokenAmount);
    }
    
    let mut flow = get_continuous_flow_or_panic(env, stream_id);
    
    // Update flow calculation first
    let current_timestamp = env.ledger().timestamp();
    update_continuous_flow(&mut flow, current_timestamp)?;
    
    // Add new balance with overflow protection
    flow.accumulated_balance = flow.accumulated_balance.saturating_add(additional_balance);
    
    // Update status if needed
    if flow.accumulated_balance > 0 && flow.flow_rate_per_second > 0 {
        flow.status = StreamStatus::Active;
    }
    
    // Store updated flow
    env.storage()
        .instance()
        .set(&DataKey::ContinuousFlow(stream_id), &flow);
    
    Ok(())
}

/// Withdraw from continuous flow with high-frequency safety
fn withdraw_from_flow(
    env: &Env,
    stream_id: u64,
    withdrawal_amount: i128,
) -> Result<i128, ContractError> {
    if withdrawal_amount <= 0 {
        return Err(ContractError::InvalidTokenAmount);
    }
    
    let mut flow = get_continuous_flow_or_panic(env, stream_id);
    
    // Update flow calculation first
    let current_timestamp = env.ledger().timestamp();
    update_continuous_flow(&mut flow, current_timestamp)?;
    
    // Check if sufficient balance available
    if flow.accumulated_balance < withdrawal_amount {
        return Err(ContractError::InvalidTokenAmount);
    }
    
    // Perform withdrawal
    flow.accumulated_balance = flow.accumulated_balance.saturating_sub(withdrawal_amount);
    
    // Update status if depleted
    if flow.accumulated_balance == 0 {
        flow.status = StreamStatus::Depleted;
    }
    
    // Store updated flow
    env.storage()
        .instance()
        .set(&DataKey::ContinuousFlow(stream_id), &flow);
    
    Ok(withdrawal_amount)
}

#[contractimpl]
impl UtilityContract {
    pub fn get_minimum_balance_to_flow() -> i128 {
        MINIMUM_BALANCE_TO_FLOW
    }

    pub fn set_oracle(env: Env, oracle_address: Address) {
        // This should be called by admin to set the oracle address
        env.storage()
            .instance()
            .set(&DataKey::Oracle, &oracle_address);
    }

    pub fn set_maintenance_config(env: Env, wallet: Address, fee_bps: i128) {
        env.storage()
            .instance()
            .set(&DataKey::MaintenanceWallet, &wallet);
        env.storage()
            .instance()
            .set(&DataKey::ProtocolFeeBps, &fee_bps);
    }

    pub fn add_supported_token(env: Env, token: Address) {
        env.storage()
            .instance()
            .set(&DataKey::SupportedToken(token), &true);
    }

    pub fn remove_supported_token(env: Env, token: Address) {
        env.storage()
            .instance()
            .set(&DataKey::SupportedToken(token), &false);
    }

    /// Add a supported withdrawal token for path payments
    pub fn add_supported_withdrawal_token(env: Env, token: Address) {
        env.storage().instance().set(&DataKey::SupportedWithdrawalToken(token), &true);
    }

    /// Remove a supported withdrawal token for path payments
    pub fn remove_supported_withdrawal_token(env: Env, token: Address) {
        env.storage().instance().set(&DataKey::SupportedWithdrawalToken(token), &false);
    }

    // ==================== ISSUE #130: GRANT STREAM INTEGRATION ====================

    /// Create a new conservation goal for a provider
    pub fn create_conservation_goal(
        env: Env,
        provider: Address,
        target_water_savings: i128,
        deadline: u64,
        grant_amount: i128,
        grant_token: Address,
    ) -> u64 {
        provider.require_auth();

        if target_water_savings <= 0 {
            panic_with_error!(&env, ContractError::InvalidGrantAmount);
        }

        if grant_amount <= 0 {
            panic_with_error!(&env, ContractError::InvalidGrantAmount);
        }

        // Generate unique goal ID
        let goal_count: u64 = env.storage()
            .instance()
            .get(&DataKey::Count)
            .unwrap_or(0);
        let goal_id = goal_count + 1;

        let now = env.ledger().timestamp();

        let goal = ConservationGoal {
            goal_id,
            provider: provider.clone(),
            target_water_savings,
            current_savings: 0,
            deadline,
            is_active: true,
            grant_amount,
            grant_token: grant_token.clone(),
            created_at: now,
            achieved_at: None,
        };

        env.storage().instance().set(&DataKey::ConservationGoal(goal_id), &goal);
        env.storage().instance().set(&DataKey::Count, &goal_id);

        // Emit goal creation event
        env.events().publish(
            (symbol_short!("GoalCr"), goal_id),
            (provider, target_water_savings, deadline, grant_amount),
        );

        goal_id
    }

    /// Update water savings for a conservation goal
    pub fn update_water_savings(env: Env, goal_id: u64, additional_savings: i128) {
        if additional_savings <= 0 {
            panic_with_error!(&env, ContractError::InvalidUsageValue);
        }

        let mut goal: ConservationGoal = env.storage()
            .instance()
            .get(&DataKey::ConservationGoal(goal_id))
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ConservationGoalNotFound));

        goal.provider.require_auth();

        if !goal.is_active {
            panic_with_error!(&env, ContractError::GoalAlreadyAchieved);
        }

        let now = env.ledger().timestamp();
        if now > goal.deadline {
            goal.is_active = false;
            env.storage().instance().set(&DataKey::ConservationGoal(goal_id), &goal);
            panic_with_error!(&env, ContractError::GoalExpired);
        }

        goal.current_savings += additional_savings;

        // Check if goal is achieved
        if goal.current_savings >= goal.target_water_savings {
            goal.is_active = false;
            goal.achieved_at = Some(now);

            // Create GoalReached event
            let goal_event = GoalReachedEvent {
                goal_id,
                provider: goal.provider.clone(),
                water_savings: goal.current_savings,
                grant_amount: goal.grant_amount,
                grant_token: goal.grant_token.clone(),
                achieved_at: now,
            };

            // Emit GoalReached event
            env.events().publish(
                (symbol_short!("GoalRch"), goal_id),
                (goal.provider.clone(), goal.current_savings, goal.grant_amount),
            );

            // Notify Grant Stream contract if configured
            if let Some(grant_stream_address) = env.storage().instance().get::<_, Address>(&DataKey::GrantStreamMatch(goal_id, goal.provider.clone())) {
                let grant_stream_client = GrantStreamClient::new(&env, &grant_stream_address);
                grant_stream_client.on_goal_reached(goal_event);
            }
        }

        env.storage().instance().set(&DataKey::ConservationGoal(goal_id), &goal);
    }

    /// Configure Grant Stream contract to listen for goal achievements
    pub fn configure_grant_stream_match(env: Env, goal_id: u64, grant_stream_contract: Address) {
        let goal: ConservationGoal = env.storage()
            .instance()
            .get(&DataKey::ConservationGoal(goal_id))
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ConservationGoalNotFound));

        goal.provider.require_auth();

        env.storage().instance().set(&DataKey::GrantStreamMatch(goal_id, goal.provider.clone()), &grant_stream_contract);

        env.events().publish(
            (symbol_short!("GrantCfg"), goal_id),
            (goal.provider.clone(), grant_stream_contract),
        );
    }

    /// Get conservation goal details
    pub fn get_conservation_goal(env: Env, goal_id: u64) -> ConservationGoal {
        env.storage()
            .instance()
            .get(&DataKey::ConservationGoal(goal_id))
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ConservationGoalNotFound))
    }

    /// Get all active conservation goals for a provider
    pub fn get_provider_conservation_goals(env: Env, provider: Address) -> Vec<u64> {
        let mut goal_ids = Vec::new(&env);
        let count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);

        for goal_id in 1..=count {
            if let Some(goal) = env.storage().instance().get::<_, ConservationGoal>(&DataKey::ConservationGoal(goal_id)) {
                if goal.provider == provider && goal.is_active {
                    goal_ids.push_back(goal_id);
                }
            }
        }

        goal_ids
    }

    /// Check if a goal has been achieved and trigger grant if needed
    pub fn check_and_trigger_grant(env: Env, goal_id: u64) {
        let goal: ConservationGoal = env.storage()
            .instance()
            .get(&DataKey::ConservationGoal(goal_id))
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ConservationGoalNotFound));

        if goal.current_savings >= goal.target_water_savings && goal.is_active {
            // Goal should have been triggered, manually trigger now
            let mut updated_goal = goal;
            let now = env.ledger().timestamp();
            updated_goal.is_active = false;
            updated_goal.achieved_at = Some(now);

            let goal_event = GoalReachedEvent {
                goal_id,
                provider: goal.provider.clone(),
                water_savings: goal.current_savings,
                grant_amount: goal.grant_amount,
                grant_token: goal.grant_token.clone(),
                achieved_at: now,
            };

            // Emit GoalReached event
            env.events().publish(
                (symbol_short!("GoalRch"), goal_id),
                (goal.provider.clone(), goal.current_savings, goal.grant_amount),
            );

            // Notify Grant Stream contract if configured
            if let Some(grant_stream_address) = env.storage().instance().get::<_, Address>(&DataKey::GrantStreamMatch(goal_id, goal.provider.clone())) {
                let grant_stream_client = GrantStreamClient::new(&env, &grant_stream_address);
                grant_stream_client.on_goal_reached(goal_event);
            }

            env.storage().instance().set(&DataKey::ConservationGoal(goal_id), &updated_goal);
        }
    }

    /// Set green energy discount for a specific meter (in basis points)
    pub fn set_green_energy_discount(env: Env, meter_id: u64, discount_bps: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        if discount_bps < 0 || discount_bps > 10000 {
            panic_with_error!(&env, ContractError::InvalidUsageValue);
        }

        meter.green_energy_discount_bps = discount_bps;
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    // ============================================================================
    // Streaming-Limit Circuit Breaker Admin Functions
    // ============================================================================

    /// Configure velocity limit parameters (admin only)
    /// 
    /// Parameters:
    /// - `admin`: Admin address that must authorize this change
    /// - `global_limit`: Maximum system-wide outflow per 24 hours
    /// - `per_stream_limit`: Maximum per-meter outflow per 24 hours
    /// - `is_enabled`: Whether velocity limiting is active
    pub fn set_velocity_limit_config(
        env: Env,
        admin: Address,
        global_limit: i128,
        per_stream_limit: i128,
        is_enabled: bool,
    ) {
        admin.require_auth();
        
        // Validate limits
        if global_limit <= 0 || per_stream_limit <= 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        if per_stream_limit > global_limit {
            panic_with_error!(&env, ContractError::VelocityLimitBreach);
        }
        
        let config = velocity_limit::VelocityConfig {
            global_limit,
            per_stream_limit,
            is_enabled,
            admin_multisig: admin.clone(),
        };
        
        set_velocity_config(&env, admin, config);
    }

    /// Apply override to suspend velocity limits (admin multi-sig only)
    /// 
    /// Parameters:
    /// - `admin`: Admin multi-sig address
    /// - `meter_id`: Meter to override (0 for global override)
    /// - `expires_at`: When override expires (0 = never expires)
    /// - `reason`: Reason code for audit trail (e.g., "false_positive", "maintenance")
    pub fn apply_velocity_override(
        env: Env,
        admin: Address,
        meter_id: u64,
        expires_at: u64,
        reason: Symbol,
    ) {
        apply_override(&env, admin, meter_id, expires_at, reason);
    }

    /// Revoke an active velocity override (admin only)
    /// 
    /// Parameters:
    /// - `admin`: Admin address
    /// - `meter_id`: Meter override to revoke (0 for global override)
    pub fn revoke_velocity_override(env: Env, admin: Address, meter_id: u64) {
        admin.require_auth();
        revoke_override(&env, meter_id);
    }

    /// Get current velocity limit configuration
    pub fn get_velocity_limits(env: Env) -> Option<velocity_limit::VelocityConfig> {
        get_velocity_config(&env)
    }

    pub fn register_meter(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        device_public_key: BytesN<32>,
        priority_index: u32,
    ) -> u64 {
        Self::register_meter_with_mode(
            env,
            user,
            provider,
            off_peak_rate,
            token,
            BillingType::PrePaid,
            device_public_key,
            priority_index,
        )
    }

    pub fn register_with_referral(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        device_public_key: BytesN<32>,
        referrer: Address,
        priority_index: u32,
    ) -> u64 {
        let meter_id = Self::register_meter(
            env.clone(),
            user.clone(),
            provider,
            off_peak_rate,
            token,
            device_public_key,
            priority_index,
        );

        if referrer != user {
            let mut meter = get_meter_or_panic(&env, meter_id);
            // Reward the new user
            meter.balance = meter.balance.saturating_add(REFERRAL_REWARD_UNITS);
            env.storage()
                .instance()
                .set(&DataKey::Meter(meter_id), &meter);

            // Reward the referrer if they have a meter? (simplified for now: just record it)
            env.storage()
                .instance()
                .set(&DataKey::Referral(user.clone()), &referrer.clone());

            env.events().publish(
                (symbol_short!("Referral"), meter_id), (referrer.clone(), user.clone()),
            );
        }

        meter_id
    }

    pub fn register_meter_with_mode(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        billing_type: BillingType,
        device_public_key: BytesN<32>,
        priority_index: u32,
    ) -> u64 {
        user.require_auth();
        
        let mut count = env.storage().instance().get::<DataKey, u64>(&DataKey::Count).unwrap_or(0);
        count += 1;

        let mut active_count = env.storage().instance().get::<_, u32>(&DataKey::ActiveMetersCount).unwrap_or(0);
        active_count += 1;

        let now = env.ledger().timestamp();
        let peak_rate = off_peak_rate.saturating_mul(PEAK_RATE_MULTIPLIER) / RATE_PRECISION;

        let meter = Meter {
            user,
            provider,
            billing_type: BillingType::PrePaid,
            off_peak_rate,
            peak_rate,
            rate_per_unit: off_peak_rate,
            balance: 0,
            debt: 0,
            last_update: now,
            is_active: true,
            token,
            usage_data: UsageData {
                total_watt_hours: 0,
                current_cycle_watt_hours: 0,
                peak_usage_watt_hours: 0,
                last_reading_timestamp: now,
                precision_factor: 1,
                renewable_watt_hours: 0,
                renewable_percentage: 0,
                monthly_volume: 0,
                last_volume_reset: now,
            },
            device_public_key,
            end_date,
            rent_deposit,
            priority_index,
            green_energy_discount_bps: 0,
            is_paused: false,
            is_disputed: false,
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        count
    }

    pub fn deduct_units_signed(env: Env, signed_data: SignedUsageData) {
        let mut meter = env.storage().instance().get::<DataKey, Meter>(&DataKey::Meter(signed_data.meter_id)).unwrap();
        meter.provider.require_auth();

        // Security Checks
        if meter.is_disputed || meter.is_paused { panic_with_error!(&env, ContractError::InDispute); }

        let now = env.ledger().timestamp();
        let effective_rate = get_effective_rate(&env, &meter, now);

        // Track providers initialized to avoid duplicate initialization
        let mut providers_initialized: Vec<Address> = Vec::new(&env);

        for meter_info in meter_infos.iter() {
            count += 1;

            let provider_clone = meter_info.provider.clone();
            let peak_rate = meter_info
                .off_peak_rate
                .saturating_mul(PEAK_RATE_MULTIPLIER)
                / RATE_PRECISION;

            let usage_data = UsageData {
                total_watt_hours: 0,
                current_cycle_watt_hours: 0,
                peak_usage_watt_hours: 0,
                last_reading_timestamp: now,
                precision_factor: 1000,
                renewable_watt_hours: 0,
                renewable_percentage: 0,
                monthly_volume: 0,
                last_volume_reset: now,
            };

            let meter = Meter {
                user: meter_info.user.clone(),
                provider: provider_clone.clone(),
                billing_type: meter_info.billing_type,
                off_peak_rate: meter_info.off_peak_rate,
                peak_rate,
                rate_per_second: meter_info.off_peak_rate,
                rate_per_unit: meter_info.off_peak_rate,
                green_energy_discount_bps: 0,
                balance: 0,
                debt: 0,
                collateral_limit: 0,
                last_update: now,
                is_active: false,
                token: meter_info.token.clone(),
                usage_data,
                max_flow_rate_per_hour: meter_info
                    .off_peak_rate
                    .saturating_mul(HOUR_IN_SECONDS as i128),
                last_claim_time: now,
                claimed_this_hour: 0,
                heartbeat: now,
                device_public_key: meter_info.device_public_key,
                is_paired: false,
                grace_period_start: 0,
                is_paused: false,
                tier_threshold: 100_000,
                tier_rate: meter_info.off_peak_rate.saturating_mul(120) / 100,
                is_disputed: false,
                challenge_timestamp: 0,
                credit_drip_rate: 0,
                is_closed: false,
            };

            env.storage().instance().set(&DataKey::Meter(count), &meter);

            // Initialize provider total pool only once per provider
            let mut already_initialized = false;
            for provider in providers_initialized.iter() {
                if provider.clone() == provider_clone {
                    already_initialized = true;
                    break;
                }
            }

            if !already_initialized {
                let current_pool = get_provider_total_pool_impl(&env, &provider_clone);
                env.storage().instance().set(
                    &DataKey::ProviderTotalPool(provider_clone.clone()),
                    &current_pool,
                );
                providers_initialized.push_back(provider_clone);
            }
        }

        // Update the global count
        env.storage().instance().set(&DataKey::Count, &count);

        let batch_event = BatchCreatedEvent {
            start_id,
            end_id: count,
            count: count - start_id + 1,
        };

        // Emit single BatchCreated event
        env.events().publish(
            (symbol_short!("BatchCr"),),
            (batch_event.start_id, batch_event.end_id, batch_event.count),
        );
        batch_event
    }

    pub fn top_up(env: Env, meter_id: u64, amount: i128, contributor: Address) {
        let mut meter = get_meter_or_panic(&env, meter_id);

        // Authorization: either the primary user OR an authorized contributor
        let is_authorized = if contributor == meter.user {
            contributor.require_auth();
            true
        } else {
            let auth_key = DataKey::AuthorizedContributor(meter_id, contributor.clone());
            if env.storage().instance().get::<_, bool>(&auth_key).unwrap_or(false) {
                contributor.require_auth();
                true
            } else {
                false
            }
        };

        if !is_authorized {
            panic_with_error!(&env, ContractError::UnauthorizedContributor);
        }

        let was_active = meter.is_active;
        let old_meter_value = provider_meter_value(&meter);
        // Transfer tokens from contributor to contract
        let token_client = token::Client::new(&env, &meter.token);
        token_client.transfer(&contributor, &env.current_contract_address(), &amount);

        // Track individual contribution
        let contribution_key = DataKey::Contributor(meter_id, contributor.clone());
        let current_contribution = env.storage().instance().get::<_, i128>(&contribution_key).unwrap_or(0);
        env.storage().instance().set(&contribution_key, &current_contribution.saturating_add(amount));

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
                // Auto-deduct debt first if in debt mode
                if meter.balance < 0 {
                    let debt_settlement = converted_amount.min(meter.balance.abs());
                    meter.balance = meter.balance.saturating_add(debt_settlement);
                    let remaining_amount = converted_amount.saturating_sub(debt_settlement);
                    meter.balance = meter.balance.saturating_add(remaining_amount);
                } else {
                    meter.balance = meter.balance.saturating_add(converted_amount);
                }
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
        refresh_activity(&mut meter, now);

        if !was_active && meter.is_active {
            meter.last_update = now;
            publish_active_event(&env, meter_id, now);
        }

        // Update provider total pool
        let new_meter_value = provider_meter_value(&meter);
        update_provider_total_pool(&env, &meter.provider, old_meter_value, new_meter_value);

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);

        // Emit conversion event
        env.events().publish(
            (symbol_short!("TokUp"), meter_id),
            (amount, converted_amount),
        );
    }

    pub fn initiate_pairing(env: Env, meter_id: u64) -> BytesN<32> {
        let meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        if meter.is_paired {
            panic_with_error!(env, ContractError::PairingAlreadyComplete);
        }

        // Generate a pseudo-random challenge using contract context and ledger info
        let challenge_data = PairingChallengeData {
            contract: env.current_contract_address(),
            meter_id,
            timestamp: env.ledger().timestamp(),
        };

        let challenge = env.crypto().sha256(&challenge_data.to_xdr(&env));

        env.storage()
            .instance()
            .set(&DataKey::PairingChallenge(meter_id), &challenge);

        env.events()
            .publish((symbol_short!("PairIn"), meter_id), challenge.clone());

        challenge.into()
    }

    pub fn complete_pairing(env: Env, meter_id: u64, signature: BytesN<64>) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        let challenge: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::PairingChallenge(meter_id))
            .unwrap_or_else(|| panic_with_error!(&env, ContractError::ChallengeNotFound));

        // Create the message that was signed
        let pairing_data = PairingChallengeData {
            contract: env.current_contract_address(),
            meter_id,
            timestamp: env.ledger().timestamp(),
        };

        // Verify the signature
        #[cfg(not(test))]
        env.crypto().ed25519_verify(
            &meter.device_public_key,
            &pairing_data.to_xdr(&env),
            &signature,
        );

        // Clear the challenge
        env.storage()
            .instance()
            .remove(&DataKey::PairingChallenge(meter_id));

        meter.is_paired = true;
        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);

        env.events()
            .publish((symbol_short!("PairComp"), meter_id), signature);
    }

    pub fn deduct_units(env: Env, signed_data: SignedUsageData) {
        let mut meter = get_meter_or_panic(&env, signed_data.meter_id);
        meter.provider.require_auth();

        // Verify the signature and pairing
        if let Err(e) = verify_usage_signature(&env, &signed_data, &meter) {
            panic_with_error!(&env, e);
        }

        // Task #88: Kill-Switch Check
        if meter.is_disputed {
            panic_with_error!(&env, ContractError::InDispute);
        }

        // Store old meter value for pool update
        let old_meter_value = provider_meter_value(&meter);

        if !meter.is_paired {
            panic_with_error!(&env, ContractError::MeterNotPaired);
        }

        let now = env.ledger().timestamp();
        let effective_rate = get_effective_rate(&meter, signed_data.timestamp);

        // Apply green energy discount if applicable
        let discounted_rate = if signed_data.is_renewable_energy && meter.green_energy_discount_bps > 0 {
            effective_rate.saturating_mul(10000 - meter.green_energy_discount_bps) / 10000
        } else {
            effective_rate
        };

        let cost = signed_data.units_consumed.saturating_mul(discounted_rate);

        // Apply provider withdrawal limits
        let mut window = apply_provider_withdrawal_limit(&env, &meter.provider, cost);

        // Task #3: Allocate to maintenance fund (0.01% = 1 basis point)
        allocate_to_maintenance_fund(&env, signed_data.meter_id, cost);

        // Task #2: Tax Compliance - Split tax before provider payout
        let tax_rate_bps = get_tax_rate_or_default(&env);
        let (tax_amount, after_tax_amount) = calculate_tax_split(cost, tax_rate_bps);

        if tax_amount > 0 {
            // Transfer tax to government vault if configured
            if let Some(gov_vault) = get_government_vault_or_default(&env) {
                let client = token::Client::new(&env, &meter.token);
                client.transfer(&env.current_contract_address(), &gov_vault, &tax_amount);

                // Emit TaxReceipt event
                let tax_receipt = TaxReceipt {
                    meter_id: signed_data.meter_id,
                    total_amount: cost,
                    tax_amount,
                    net_amount: after_tax_amount,
                    tax_rate_bps,
                    government_vault: gov_vault.clone(),
                    timestamp: now,
                };
                env.events().publish(
                    (soroban_sdk::symbol_short!("TaxRec"), signed_data.meter_id),
                    tax_receipt,
                );
            }
        }

        // Apply the claim (using after-tax amount for actual provider payout)
        apply_provider_claim(&env, &mut meter, after_tax_amount);

        // Update provider window
        window.daily_withdrawn = window.daily_withdrawn.saturating_add(cost);
        env.storage()
            .instance()
            .set(&DataKey::ProviderWindow(meter.provider.clone()), &window);

        // Update usage data
        meter.usage_data.total_watt_hours = meter
            .usage_data
            .total_watt_hours
            .saturating_add(signed_data.watt_hours_consumed);
        meter.usage_data.current_cycle_watt_hours = meter
            .usage_data
            .current_cycle_watt_hours
            .saturating_add(signed_data.watt_hours_consumed);

        // Track renewable energy usage
        if signed_data.is_renewable_energy {
            meter.usage_data.renewable_watt_hours = meter
                .usage_data
                .renewable_watt_hours
                .saturating_add(signed_data.watt_hours_consumed);
        }

        // Update renewable percentage
        if meter.usage_data.total_watt_hours > 0 {
            meter.usage_data.renewable_percentage = meter
                .usage_data
                .renewable_watt_hours
                .saturating_mul(10000) / meter.usage_data.total_watt_hours; // in basis points
        }

        if meter.usage_data.current_cycle_watt_hours > meter.usage_data.peak_usage_watt_hours {
            meter.usage_data.peak_usage_watt_hours = meter.usage_data.current_cycle_watt_hours;
        }

        // Update activity status with grace period logic
        refresh_activity(&mut meter, now);

        meter.last_update = now;

        // Task #3: Auto-extend TTL if needed (every 500,000 ledgers)
        auto_extend_ttl_if_needed(&env, signed_data.meter_id);

        // Task #89: Update monthly volume
        let now = env.ledger().timestamp();
        if now.saturating_sub(meter.usage_data.last_volume_reset) >= (30 * DAY_IN_SECONDS) {
            meter.usage_data.monthly_volume = cost;
            meter.usage_data.last_volume_reset = now;
        } else {
            meter.usage_data.monthly_volume = meter.usage_data.monthly_volume.saturating_add(cost);
        }

        // Update provider total pool
        let new_meter_value = provider_meter_value(&meter);
        update_provider_total_pool(&env, &meter.provider, old_meter_value, new_meter_value);

        env.storage()
            .instance()
            .set(&DataKey::Meter(signed_data.meter_id), &meter);

        // Emit UsageReported event
        env.events().publish(
            (Symbol::new(&env, "UsageReported"), signed_data.meter_id),
            (signed_data.units_consumed, cost),
        );
    }

    pub fn claim(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        // Task #88: Kill-Switch Check
        if meter.is_disputed {
            panic_with_error!(&env, ContractError::InDispute);
        }

        // Store old meter value for pool update
        let old_meter_value = provider_meter_value(&meter);

        let now = env.ledger().timestamp();
        let elapsed = now.checked_sub(meter.last_update).unwrap_or(0);

        // Task #90: Credit Settlement Flow
        // If there's a credit_drip_rate, add it to the normal consumption flow
        let amount = (elapsed as i128).saturating_mul(meter.rate_per_unit.saturating_add(meter.credit_drip_rate));

        // Check if we're in the same hour as last claim
        let current_hour = now / 3600;
        let last_claim_hour = meter.last_claim_time / 3600;

        if current_hour == last_claim_hour {
            // Same hour, check if we exceed max flow rate
            let max_allowed = meter.max_flow_rate_per_hour - meter.claimed_this_hour;
            let actual_amount = if amount > max_allowed {
                max_allowed
            } else {
                amount
            };

            // Ensure we don't exceed debt threshold
            let claimable = if actual_amount > meter.balance
                && meter.balance - actual_amount >= DEBT_THRESHOLD
            {
                actual_amount
            } else if actual_amount > meter.balance {
                meter.balance - DEBT_THRESHOLD // Allow going down to threshold
            } else {
                actual_amount
            };

            if claimable > 0 {
                let client = token::Client::new(&env, &meter.token);
                let mut payout = claimable;

                // Task #3: Allocate to maintenance fund (0.01% = 1 basis point)
                allocate_to_maintenance_fund(&env, meter_id, claimable);

                // Task #2: Tax Compliance - Split tax before provider payout
                let tax_rate_bps = get_tax_rate_or_default(&env);
                let (tax_amount, after_tax_amount) = calculate_tax_split(payout, tax_rate_bps);

                if tax_amount > 0 {
                    // Transfer tax to government vault if configured
                    if let Some(gov_vault) = get_government_vault_or_default(&env) {
                        client.transfer(&env.current_contract_address(), &gov_vault, &tax_amount);

                        // Emit TaxReceipt event
                        let tax_receipt = TaxReceipt {
                            meter_id,
                            total_amount: claimable,
                            tax_amount,
                            net_amount: after_tax_amount,
                            tax_rate_bps,
                            government_vault: gov_vault.clone(),
                            timestamp: now,
                        };
                        env.events().publish(
                            (soroban_sdk::symbol_short!("TaxRcpt"), meter_id),
                            tax_receipt,
                        );
                    }
                }

                payout = after_tax_amount;

                // Protocol fee (existing logic)
                if let Some(wallet) = env
                    .storage()
                    .instance()
                    .get::<_, Address>(&DataKey::MaintenanceWallet)
                {
                    let fee_bps: i128 = env
                        .storage()
                        .instance()
                        .get(&DataKey::ProtocolFeeBps)
                        .unwrap_or(0);
                    let fee = (payout * fee_bps) / 10000;
                    payout -= fee;
                    if fee > 0 {
                        client.transfer(&env.current_contract_address(), &wallet, &fee);
                    }
                }
                if payout > 0 {
                    client.transfer(&env.current_contract_address(), &meter.provider, &payout);
                }
                meter.balance -= claimable;
                meter.claimed_this_hour += claimable;

                // If credit drip was active, reduce the debt if in PostPaid mode
                if meter.billing_type == BillingType::PostPaid && meter.credit_drip_rate > 0 {
                    let credit_settlement = (elapsed as i128).saturating_mul(meter.credit_drip_rate).min(meter.debt);
                    meter.debt = meter.debt.saturating_sub(credit_settlement);
                }
            }
        } else {
            // New hour, reset claimed_this_hour
            meter.claimed_this_hour = 0;

            // Ensure we don't exceed debt threshold
            let claimable = if amount > meter.balance && meter.balance - amount >= DEBT_THRESHOLD {
                amount
            } else if amount > meter.balance {
                meter.balance - DEBT_THRESHOLD // Allow going down to threshold
            } else {
                amount
            };

            if claimable > 0 {
                let client = token::Client::new(&env, &meter.token);
                let mut payout = claimable;

                // Task #3: Allocate to maintenance fund (0.01% = 1 basis point)
                allocate_to_maintenance_fund(&env, meter_id, claimable);

                // Task #2: Tax Compliance - Split tax before provider payout
                let tax_rate_bps = get_tax_rate_or_default(&env);
                let (tax_amount, after_tax_amount) = calculate_tax_split(payout, tax_rate_bps);

                if tax_amount > 0 {
                    // Transfer tax to government vault if configured
                    if let Some(gov_vault) = get_government_vault_or_default(&env) {
                        client.transfer(&env.current_contract_address(), &gov_vault, &tax_amount);

                        // Emit TaxReceipt event
                        let tax_receipt = TaxReceipt {
                            meter_id,
                            total_amount: claimable,
                            tax_amount,
                            net_amount: after_tax_amount,
                            tax_rate_bps,
                            government_vault: gov_vault.clone(),
                            timestamp: now,
                        };
                        env.events().publish(
                            (soroban_sdk::symbol_short!("TaxRcpt"), meter_id),
                            tax_receipt,
                        );
                    }
                }

                payout = after_tax_amount;

                // Protocol fee (existing logic)
                if let Some(wallet) = env
                    .storage()
                    .instance()
                    .get::<_, Address>(&DataKey::MaintenanceWallet)
                {
                    let fee_bps: i128 = env
                        .storage()
                        .instance()
                        .get(&DataKey::ProtocolFeeBps)
                        .unwrap_or(0);
                    let fee = (payout * fee_bps) / 10000;
                    payout -= fee;
                    if fee > 0 {
                        client.transfer(&env.current_contract_address(), &wallet, &fee);
                    }
                }
                if payout > 0 {
                    client.transfer(&env.current_contract_address(), &meter.provider, &payout);
                }
                meter.balance -= claimable;
                meter.claimed_this_hour = claimable;

                // If credit drip was active, reduce the debt if in PostPaid mode
                if meter.billing_type == BillingType::PostPaid && meter.credit_drip_rate > 0 {
                    let credit_settlement = (elapsed as i128).saturating_mul(meter.credit_drip_rate).min(meter.debt);
                    meter.debt = meter.debt.saturating_sub(credit_settlement);
                }
            }
        }

        meter.last_update = now;
        meter.last_claim_time = now;

        // Update activity status with grace period logic
        refresh_activity(&mut meter, now);

        // Task #3: Auto-extend TTL if needed (every 500,000 ledgers)
        auto_extend_ttl_if_needed(&env, meter_id);

        // Update provider total pool
        let new_meter_value = provider_meter_value(&meter);
        update_provider_total_pool(&env, &meter.provider, old_meter_value, new_meter_value);

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);

        env.events()
            .publish((symbol_short!("Claim"), meter_id), settlement.gross_claimed);
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
        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn reset_cycle_usage(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();
        meter.usage_data.current_cycle_watt_hours = 0;
        meter.usage_data.last_reading_timestamp = env.ledger().timestamp();
        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
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

    pub fn get_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get::<DataKey, u64>(&DataKey::Count)
            .unwrap_or(0)
    }

    pub fn get_provider_window(env: Env, provider: Address) -> Option<ProviderWithdrawalWindow> {
        env.storage()
            .instance()
            .get(&DataKey::ProviderWindow(provider))
    }

    pub fn get_provider_total_pool(env: Env, provider: Address) -> i128 {
        get_provider_total_pool_impl(&env, &provider)
    }

    pub fn get_watt_hours_display(precise_watt_hours: i128, precision_factor: i128) -> i128 {
        if precision_factor <= 0 {
            return precise_watt_hours; // Fallback to avoid division by zero
        }
        precise_watt_hours / precision_factor
    }

    pub fn calculate_expected_depletion(env: Env, meter_id: u64) -> Option<u64> {
        if let Some(meter) = env
            .storage()
            .instance()
            .get::<_, Meter>(&DataKey::Meter(meter_id))
        {
            if meter.balance <= 0 || meter.rate_per_unit <= 0 {
                return Some(0); // Already depleted or no consumption
            }

            let seconds_until_depletion = meter.balance / meter.rate_per_unit;
            let current_time = env.ledger().timestamp();
            Some(current_time + seconds_until_depletion as u64)
        } else {
            None
        }
    }

    pub fn set_meter_pause(env: Env, meter_id: u64, paused: bool) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();

        meter.is_paused = paused;
        let now = env.ledger().timestamp();
        refresh_activity(&mut meter, now);

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);

        env.events()
            .publish((symbol_short!("Paused"), meter_id), paused);
    }

    pub fn set_tiered_pricing(env: Env, meter_id: u64, threshold: i128, rate: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        meter.tier_threshold = threshold;
        meter.tier_rate = rate;

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn vote_for_asset(env: Env, voter: Address, asset_symbol: Symbol) {
        voter.require_auth();

        // Check if user already voted for this specific asset
        if env
            .storage()
            .instance()
            .has(&DataKey::UserVoted(voter.clone(), asset_symbol.clone()))
        {
            panic_with_error!(env, ContractError::AlreadyVoted);
        }

        let mut votes = env
            .storage()
            .instance()
            .get::<_, i128>(&DataKey::PollVotes(asset_symbol.clone()))
            .unwrap_or(0);

        votes += 1;

        env.storage()
            .instance()
            .set(&DataKey::PollVotes(asset_symbol.clone()), &votes);
        env.storage()
            .instance()
            .set(&DataKey::UserVoted(voter, asset_symbol.clone()), &true);

        env.events()
            .publish((symbol_short!("Voted"), asset_symbol), votes);
    }

    pub fn get_votes(env: Env, asset_symbol: Symbol) -> i128 {
        env.storage()
            .instance()
            .get::<_, i128>(&DataKey::PollVotes(asset_symbol))
            .unwrap_or(0)
    }

    pub fn emergency_shutdown(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        // Emergency shutdown always disables the meter regardless of balance
        meter.is_active = false;

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn set_max_flow_rate(env: Env, meter_id: u64, max_rate_per_hour: i128) {
        let mut meter: Meter = env
            .storage()
            .instance()
            .get(&DataKey::Meter(meter_id))
            .ok_or("Meter not found")
            .unwrap();
        meter.provider.require_auth();

        meter.max_flow_rate_per_hour = max_rate_per_hour;

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn update_heartbeat(env: Env, meter_id: u64) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.user.require_auth();
        meter.heartbeat = env.ledger().timestamp();
        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn withdraw_earnings(env: Env, meter_id: u64, amount_usd_cents: i128) {
        let mut meter = get_meter_or_panic(&env, meter_id);
        meter.provider.require_auth();

        if amount_usd_cents <= 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }

        // Store old meter value for pool update
        let old_meter_value = provider_meter_value(&meter);

        let available_earnings = match meter.billing_type {
            BillingType::PrePaid => meter.balance,
            BillingType::PostPaid => meter.debt,
        };

        if amount_usd_cents > available_earnings {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }

        // Convert USD cents to XLM if needed
        let withdrawal_amount =
            match convert_usd_to_xlm_if_needed(&env, amount_usd_cents, &meter.token) {
                Ok(amount) => amount,
                Err(_) => panic_with_error!(&env, ContractError::PriceConversionFailed),
            };

        let client = token::Client::new(&env, &meter.token);
        client.transfer(
            &env.current_contract_address(),
            &meter.provider,
            &withdrawal_amount,
        );

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
        let was_active = meter.is_active;
        refresh_activity(&mut meter, now);

        if !was_active && meter.is_active {
            meter.last_update = now;
        }

        // Update provider total pool
        let new_meter_value = provider_meter_value(&meter);
        update_provider_total_pool(&env, &meter.provider, old_meter_value, new_meter_value);

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);

        // Emit conversion event if XLM was used
        if is_native_token(&meter.token) {
            env.events().publish(
                (symbol_short!("USD2XL"), meter_id),
                (amount_usd_cents, withdrawal_amount),
            );
        }
    }

    pub fn get_current_rate(env: Env) -> Option<PriceData> {
        match env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::Oracle)
        {
            Some(oracle_address) => {
                let oracle_client = PriceOracleClient::new(&env, &oracle_address);
                Some(oracle_client.get_price())
            }
            None => None,
        }
    }

    pub fn get_provider_total_pool(env: Env, provider: Address) -> i128 {
        get_provider_total_pool_impl(&env, &provider)
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

    pub fn get_watt_hours_display(watt_hours: i128, precision_factor: i128) -> i128 {
        watt_hours / precision_factor
    }

    /// Unlink a meter from its current tenant and link it to a new tenant.
    /// All historical usage data is preserved. Requires auth from the current
    /// user, the new user, and the provider.
    pub fn transfer_meter_ownership(env: Env, meter_id: u64, new_user: Address) {
        let mut meter = get_meter_or_panic(&env, meter_id);

        meter.user.require_auth();
        meter.provider.require_auth();
        new_user.require_auth();

        let old_user = meter.user.clone();
        let old_meter_value = provider_meter_value(&meter);
        meter.user = new_user.clone();

        // Update provider total pool (provider stays the same, only user changes)
        let new_meter_value = provider_meter_value(&meter);
        update_provider_total_pool(&env, &meter.provider, old_meter_value, new_meter_value);

        env.storage()
            .instance()
            .set(&DataKey::Meter(meter_id), &meter);

        env.events()
            .publish((symbol_short!("Transfer"), meter_id), (old_user, new_user));
    }

    // Continuous Flow Engine Public Interface

    /// Create a new continuous flow stream
    pub fn create_continuous_stream(
        env: Env,
        stream_id: u64,
        flow_rate_per_second: i128,
        initial_balance: i128,
    ) {
        env.current_contract_address().require_auth();
        
        if flow_rate_per_second < 0 || initial_balance < 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        let current_timestamp = env.ledger().timestamp();
        let flow = create_continuous_flow(stream_id, flow_rate_per_second, initial_balance, current_timestamp);
        
        env.storage()
            .instance()
            .set(&DataKey::ContinuousFlow(stream_id), &flow);
        
        env.events().publish(
            symbol_short!("StreamCreated"),
            (stream_id, flow_rate_per_second, initial_balance, current_timestamp)
        );
    }

    /// Update the flow rate of an existing continuous stream
    pub fn update_continuous_flow_rate(env: Env, stream_id: u64, new_flow_rate: i128) {
        if new_flow_rate < 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        update_flow_rate(&env, stream_id, new_flow_rate).unwrap();
    }

    /// Add balance to a continuous flow stream
    pub fn add_continuous_balance(env: Env, stream_id: u64, additional_balance: i128) {
        add_balance_to_flow(&env, stream_id, additional_balance).unwrap();
        
        env.events().publish(
            symbol_short!("BalanceAdded"),
            (stream_id, additional_balance)
        );
    }

    /// Withdraw from a continuous flow stream
    pub fn withdraw_continuous(env: Env, stream_id: u64, withdrawal_amount: i128) -> i128 {
        let withdrawn = withdraw_from_flow(&env, stream_id, withdrawal_amount).unwrap();
        
        env.events().publish(
            symbol_short!("Withdrawal"),
            (stream_id, withdrawn)
        );
        
        withdrawn
    }

    /// Get the current state of a continuous flow stream
    pub fn get_continuous_flow(env: Env, stream_id: u64) -> Option<ContinuousFlow> {
        env.storage()
            .instance()
            .get::<DataKey, ContinuousFlow>(&DataKey::ContinuousFlow(stream_id))
    }

    /// Calculate expected depletion time for a continuous flow stream
    pub fn calculate_continuous_depletion(env: Env, stream_id: u64) -> Option<u64> {
        if let Some(flow) = env.storage()
            .instance()
            .get::<DataKey, ContinuousFlow>(&DataKey::ContinuousFlow(stream_id))
        {
            if flow.status != StreamStatus::Active || flow.flow_rate_per_second <= 0 {
                return None;
            }
            
            let current_timestamp = env.ledger().timestamp();
            let accumulation = calculate_flow_accumulation(&flow, current_timestamp);
            let remaining_balance = flow.accumulated_balance.saturating_sub(accumulation);
            
            if remaining_balance <= 0 {
                return Some(current_timestamp);
            }
            
            let seconds_until_depletion = remaining_balance / flow.flow_rate_per_second;
            Some(current_timestamp + seconds_until_depletion as u64)
        } else {
            None
        }
    }

    /// Pause a continuous flow stream
    pub fn pause_continuous_flow(env: Env, stream_id: u64) {
        update_flow_rate(&env, stream_id, 0).unwrap();
    }

    /// Resume a continuous flow stream with specified rate
    pub fn resume_continuous_flow(env: Env, stream_id: u64, flow_rate_per_second: i128) {
        if flow_rate_per_second <= 0 {
            panic_with_error!(&env, ContractError::InvalidTokenAmount);
        }
        
        update_flow_rate(&env, stream_id, flow_rate_per_second).unwrap();
    }

    /// Get the current accumulated balance after flow calculation
    pub fn get_continuous_balance(env: Env, stream_id: u64) -> Option<i128> {
        if let Some(mut flow) = env.storage()
            .instance()
            .get::<DataKey, ContinuousFlow>(&DataKey::ContinuousFlow(stream_id))
        {
            let current_timestamp = env.ledger().timestamp();
            let accumulation = calculate_flow_accumulation(&flow, current_timestamp);
            let remaining_balance = flow.accumulated_balance.saturating_sub(accumulation);
            
            Some(remaining_balance)
        } else {
            None
        }
    }
}

fn verify_usage_signature(
    env: &Env,
    signed_data: &SignedUsageData,
    meter: &Meter,
) -> Result<(), ContractError> {
    // Check if the provided public key matches the registered meter's public key
    if signed_data.public_key != meter.device_public_key {
        return Err(ContractError::PublicKeyMismatch);
    }

    // Check timestamp is not too old (prevent replay attacks)
    let current_time = env.ledger().timestamp();
    if current_time.saturating_sub(signed_data.timestamp) > MAX_TIMESTAMP_DELAY {
        return Err(ContractError::TimestampTooOld);
    }

    // Create the message that was signed
    let report = UsageReport {
        meter_id: signed_data.meter_id,
        timestamp: signed_data.timestamp,
        watt_hours_consumed: signed_data.watt_hours_consumed,
        units_consumed: signed_data.units_consumed,
        is_renewable_energy: signed_data.is_renewable_energy,
    };

    // Verify the signature using Soroban's built-in signature verification.
    // In test builds, we skip the actual crypto check to allow mock signatures.
    #[cfg(not(test))]
    env.crypto().ed25519_verify(
        &signed_data.public_key,
        &report.to_xdr(&env),
        &signed_data.signature,
    );
    Ok(())
}

mod test;
