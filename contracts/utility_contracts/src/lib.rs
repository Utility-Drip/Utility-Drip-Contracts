#![no_std]
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    Address, Env, BytesN, Vec, Symbol, String,
};

// --- Constants (Merged) ---
const DEFAULT_BUFFER_DAYS: i128 = 3;
const TRUSTED_BUFFER_DAYS: i128 = 1;
const MINIMUM_BALANCE_TO_FLOW: i128 = 500;
const HOUR_IN_SECONDS: u64 = 60 * 60;
const DAY_IN_SECONDS: u64 = 24 * HOUR_IN_SECONDS;
const GRACE_PERIOD_SECONDS: u64 = 86_400;
const DEBT_THRESHOLD: i128 = -10_000_000;
const DAILY_WITHDRAWAL_PERCENT: i128 = 10;
const MAX_USAGE_PER_UPDATE: i128 = 1_000_000_000_000i128;
const MAX_TIMESTAMP_DELAY: u64 = 300;
const PEAK_HOUR_START: u64 = 18 * HOUR_IN_SECONDS;
const PEAK_HOUR_END: u64 = 21 * HOUR_IN_SECONDS;
const PEAK_RATE_MULTIPLIER: i128 = 3;
const RATE_PRECISION: i128 = 2;
const REFERRAL_REWARD_UNITS: i128 = 500;
const XLM_PRECISION: i128 = 10_000_000;
const XLM_MINIMUM_INCREMENT: i128 = 1;
const MAX_RESELLER_FEE_BPS: i128 = 2000;
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

// --- Data Structures (Merged) ---

#[contracttype]
#[derive(Clone)]
pub struct PriceData {
    pub price: i128,
    pub decimals: u32,
    pub last_updated: u64,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BillingType { PrePaid, PostPaid }

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
    // From Self-Destruct/Recovery branch
    pub end_date: u64,
    pub rent_deposit: i128,
    // From Main branch
    pub is_paired: bool,
    pub grace_period_start: u64,
    pub is_paused: bool,
    pub tier_threshold: i128,
    pub tier_rate: i128,
    pub is_disputed: bool,
    pub challenge_timestamp: u64,
    pub credit_drip_rate: i128,
    pub is_closed: bool,
    pub priority_index: u32,
    pub milestone_deadline: u64,
    pub milestone_confirmed: bool,
    pub green_energy_discount_bps: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct MultiSigConfig {
    pub provider: Address,
    pub finance_wallets: Vec<Address>,
    pub required_signatures: u32,
    pub threshold_amount: i128,
    pub is_active: bool,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct WithdrawalRequest {
    pub request_id: u64,
    pub provider: Address,
    pub meter_id: u64,
    pub amount_usd_cents: i128,
    pub destination: Address,
    pub proposer: Address,
    pub created_at: u64,
    pub expires_at: u64,
    pub approval_count: u32,
    pub is_executed: bool,
    pub is_cancelled: bool,
}

#[contracttype]
pub enum DataKey {
    Meter(u64),
    Count,
    Oracle,
    ActiveMetersCount,
    SoroSusuContract,
    VestingVault,
    NFTMinter,
    MaintenanceWallet,
    ProtocolFeeBps,
    SupportedToken(Address),
    SupportedWithdrawalToken(Address),
    ProviderTotalPool(Address),
    Referral(Address),
    PollVotes(Symbol),
    UserVoted(Address, Symbol),
    ClosingFeeBps,
    Contributor(u64, Address),
    AuthorizedContributor(u64, Address),
    GovernmentVault(Address),
    TaxRateBps,
    MaintenanceFund(u64),
    AutoExtendThreshold,
    ProposedUpgrade,
    UpgradeProposalTime,
    VetoDeadline,
    UserVetoed(Address, u64),
    CurrentAdmin,
    AdminTransferProposal,
    AdminVeto(Address, u64),
    ActiveUsers,
    ComplianceOfficer,
    LegalFreeze(u64),
    LegalVault,
    VerifiedProvider(Address),
    SubDaoConfig(Address),
    MultiSigConfig(Address),
    WithdrawalRequest(Address, u64),
    WithdrawalRequestCount(Address),
    WithdrawalApproval(Address, u64, Address),
    PairingChallenge(u64),
}

#[contracterror]
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    MeterNotFound = 1,
    OracleNotSet = 2,
    PriceConversionFailed = 4,
    InvalidTokenAmount = 5,
    InvalidUsageValue = 6,
    UsageExceedsLimit = 7,
    PublicKeyMismatch = 10,
    TimestampTooOld = 11,
    StreamNotFinished = 12,
    BalanceNotEmpty = 13,
    InDispute = 22,
    AdminTransferActive = 35,
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
}

// --- Contract Implementation ---

#[contract]
pub struct UtilityContract;

#[contractimpl]
impl UtilityContract {

    pub fn register_meter(
        env: Env,
        user: Address,
        provider: Address,
        off_peak_rate: i128,
        token: Address,
        device_public_key: BytesN<32>,
        priority_index: u32,
        end_date: u64,
        rent_deposit: i128,
    ) -> u64 {
        user.require_auth();

        if rent_deposit > 0 {
            let client = token::Client::new(&env, &token);
            client.transfer(&user, &env.current_contract_address(), &rent_deposit);
        }

        let mut count = env.storage().instance().get::<_, u64>(&DataKey::Count).unwrap_or(0);
        count += 1;

        let mut active_count = env.storage().instance().get::<_, u32>(&DataKey::ActiveMetersCount).unwrap_or(0);
        active_count += 1;

        let now = env.ledger().timestamp();
        let peak_rate = off_peak_rate.saturating_mul(PEAK_RATE_MULTIPLIER) / RATE_PRECISION;

        let usage_data = UsageData {
            total_watt_hours: 0,
            current_cycle_watt_hours: 0,
            peak_usage_watt_hours: 0,
            last_reading_timestamp: now,
            precision_factor: 1,
            renewable_watt_hours: 0,
            renewable_percentage: 0,
            monthly_volume: 0,
            last_volume_reset: now,
        };

        let meter = Meter {
            user: user.clone(),
            provider: provider.clone(),
            billing_type: BillingType::PrePaid,
            off_peak_rate,
            peak_rate,
            rate_per_unit: off_peak_rate,
            balance: 0,
            debt: 0,
            collateral_limit: 0,
            last_update: now,
            is_active: true,
            token,
            usage_data,
            max_flow_rate_per_hour: 0,
            last_claim_time: 0,
            claimed_this_hour: 0,
            heartbeat: now,
            device_public_key,
            end_date,
            rent_deposit,
            is_paired: false,
            grace_period_start: 0,
            is_paused: false,
            tier_threshold: 0,
            tier_rate: 0,
            is_disputed: false,
            challenge_timestamp: 0,
            credit_drip_rate: 0,
            is_closed: false,
            priority_index,
            milestone_deadline: 0,
            milestone_confirmed: false,
            green_energy_discount_bps: 0,
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        env.storage().instance().set(&DataKey::ActiveMetersCount, &active_count);

        count
    }

    pub fn finalize_and_purge(env: Env, meter_id: u64) {
        let meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).expect("Meter Not Found");
        meter.user.require_auth();

        let now = env.ledger().timestamp();

        if now < meter.end_date {
            panic_with_error!(&env, ContractError::StreamNotFinished);
        }

        if meter.balance > 0 || meter.debt > 0 {
            panic_with_error!(&env, ContractError::BalanceNotEmpty);
        }

        if meter.rent_deposit > 0 {
            let client = token::Client::new(&env, &meter.token);
            client.transfer(&env.current_contract_address(), &meter.user, &meter.rent_deposit);
        }

        env.storage().instance().remove(&DataKey::Meter(meter_id));

        let mut active_count = env.storage().instance().get::<_, u32>(&DataKey::ActiveMetersCount).unwrap_or(0);
        if active_count > 0 {
            active_count -= 1;
            env.storage().instance().set(&DataKey::ActiveMetersCount, &active_count);
        }

        if active_count == 0 {
            env.storage().instance().remove(&DataKey::Count);
            env.storage().instance().remove(&DataKey::Oracle);
        }

        env.events().publish((symbol_short!("Purge"), meter_id), now);
    }
    
    // ... Additional logic for Multi-Sig, Throttling, and Deductions should be merged here following the same pattern ...
}