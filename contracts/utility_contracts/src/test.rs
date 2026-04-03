#![cfg(test)]
#![allow(deprecated)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{token, Address, BytesN, Env, Vec};

// --- Helpers ---
fn device_key(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn create_token(env: &Env) -> Address {
    let admin = Address::generate(env);
    env.register_stellar_asset_contract_v2(admin).address()
}

// ==================== MOCK CONTRACTS ====================

mod mock_sorosusu {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct MockSoroSusu;

    #[contractimpl]
    impl MockSoroSusu {
        pub fn set_default(env: Env, user: Address, in_default: bool) {
            env.storage().instance().set(&user, &in_default);
        }

        pub fn is_in_default(env: Env, user: Address) -> bool {
            env.storage().instance().get(&user).unwrap_or(false)
        }

        pub fn is_trusted_saver(_env: Env, _user: Address) -> bool { false }
        pub fn get_susu_score(_env: Env, _user: Address) -> u32 { 0 }

        pub fn record_debt_payment(env: Env, user: Address, amount: i128) {
            let key = (user.clone(), soroban_sdk::symbol_short!("paid"));
            let current: i128 = env.storage().instance().get(&key).unwrap_or(0);
            env.storage().instance().set(&key, &current.saturating_add(amount));
        }
    }
}

#[test]
fn test_grace_period_expiration() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);

    // Top up with minimum balance to activate
    client.top_up(&meter_id, &500);
    let meter = client.get_meter(&meter_id).unwrap();
    assert!(meter.is_active);
    assert_eq!(meter.balance, 500);
    assert_eq!(meter.grace_period_start, 0);

    // Pair the meter
    client.initiate_pairing(&meter_id);
    client.complete_pairing(&meter_id, &BytesN::from_array(&env, &[2u8; 64]));

    // Use up balance exactly to 0 - should start grace period
    env.ledger().set_timestamp(env.ledger().timestamp() + 50); // 50 seconds * 10 rate = 500
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 0);
    assert!(meter.is_active); // Should still be active due to grace period
    assert!(meter.grace_period_start > 0); // Grace period should have started

    // Use some more to go into debt (but above threshold)
    env.ledger().set_timestamp(env.ledger().timestamp() + 10); // 10 seconds * 10 rate = 100
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, -100);
    assert!(meter.is_active); // Should still be active during grace period

    // Fast forward 23 hours - should still be active
    env.ledger().set_timestamp(env.ledger().timestamp() + (23 * 60 * 60));
    client.claim(&meter_id); // This will trigger grace period check

    let meter = client.get_meter(&meter_id).unwrap();
    assert!(meter.is_active); // Should still be active (less than 24 hours)

    // Fast forward another 2 hours (total 25 hours) - should expire grace period
    env.ledger().set_timestamp(env.ledger().timestamp() + (2 * 60 * 60));
    client.claim(&meter_id); // This will trigger grace period check

    let meter = client.get_meter(&meter_id).unwrap();
    assert!(!meter.is_active); // Should be inactive (grace period expired)
    assert!(meter.balance < 0); // Should still be in debt
}

#[test]
fn test_grace_period_debt_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Mint enough to test debt threshold
    token_admin_client.mint(&user, &20_000_000); // 2 XLM

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &1_000_000, &token_address, &device_public_key); // High rate

    // Top up with small amount
    client.top_up(&meter_id, &1_000_000);
    let meter = client.get_meter(&meter_id).unwrap();
    assert!(meter.is_active);

    // Pair the meter
    client.initiate_pairing(&meter_id);
    client.complete_pairing(&meter_id, &BytesN::from_array(&env, &[2u8; 64]));

    // Try to claim beyond debt threshold (-10 XLM = -10,000,000 stroops)
    env.ledger().set_timestamp(env.ledger().timestamp() + 15); // 15 seconds * 1,000,000 rate = 15,000,000
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, -10_000_000); // Should stop at debt threshold
    assert!(meter.is_active); // Should be in grace period

    // Try to claim more - should be blocked by threshold
    env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, -10_000_000); // Should not go below threshold
}

#[test]
fn test_auto_debt_settlement_on_top_up() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &2000);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);

    // Top up and use up balance to go into debt
    client.top_up(&meter_id, &1000);
    env.ledger().set_timestamp(env.ledger().timestamp() + 150); // 150 seconds * 10 rate = 1500
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, -500);
    assert!(meter.is_active); // Should be in grace period

    // Top up - should auto-settle debt first
    client.top_up(&meter_id, &800);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 300); // 800 - 500 debt settlement = 300 remaining
    assert!(meter.is_active); // Should be active with positive balance
    assert_eq!(meter.grace_period_start, 0); // Grace period should be reset
}

#[test]
fn test_peak_hour_tariff() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Initial funding
    token_admin_client.mint(&user, &5000);

    // Register Meter
    let rate = 10; // 10 tokens per unit
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id =
        client.register_meter(&user, &provider, &rate, &token_address, &device_public_key);

    // Pair Meter
    let challenge = client.initiate_pairing(&meter_id);
    let signature = BytesN::from_array(&env, &[2u8; 64]);
    client.complete_pairing(&meter_id, &signature);

    client.top_up(&meter_id, &5000);

    // Set time to 19:00:00 UTC (19 * 3600 = 68400)
    // 19:00 falls exactly in the 18:00 - 21:00 peak hours bracket
    env.ledger().set_timestamp(68400);

    // Consume 10 units. Base cost = 10 * 10 = 100 tokens.
    // 150% Peak multiplier means 150 tokens claimed.
    let signed_data = SignedUsageData {
        meter_id,
        timestamp: 68400,
        watt_hours_consumed: 1000,
        units_consumed: 10,
        signature: BytesN::from_array(&env, &[3u8; 64]),
        public_key: device_public_key,
    };
    client.deduct_units(&signed_data);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 4850); // 5000 - 150
    assert_eq!(token.balance(&provider), 150);
}

#[test]
fn test_calculate_expected_depletion() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);
    client.top_up(&meter_id, &500);

    // Calculate depletion time
    let depletion_time = client.calculate_expected_depletion(&meter_id).unwrap();
    let current_time = env.ledger().timestamp();
    assert_eq!(depletion_time, current_time + 50);
}

#[test]
fn test_emergency_shutdown() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);
    client.top_up(&meter_id, &500);

    let meter = client.get_meter(&meter_id).unwrap();
    assert!(meter.is_active);

    client.emergency_shutdown(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    assert!(!meter.is_active);
}

#[test]
fn test_heartbeat_functionality() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);

    assert!(!client.is_meter_offline(&meter_id));

    env.ledger().set_timestamp(env.ledger().timestamp() + 3700);
    assert!(client.is_meter_offline(&meter_id));

    client.update_heartbeat(&meter_id);
    assert!(!client.is_meter_offline(&meter_id));
}

#[test]
fn test_claim_within_daily_limit_tracks_withdrawn() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &10000);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);
    client.top_up(&meter_id, &5000);

    env.ledger().set_timestamp(env.ledger().timestamp() + 5);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    let provider_window = client.get_provider_window(&provider).unwrap();

    assert_eq!(meter.balance, 4950);
    assert_eq!(token.balance(&provider), 50);
    assert_eq!(token.balance(&contract_id), 4950);
    assert_eq!(provider_window.daily_withdrawn, 50);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_claim_reverts_when_daily_limit_is_exceeded() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_public_key);
    client.top_up(&meter_id, &500);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 10_000);
    client.claim(&meter_id);
}

#[test]
fn test_provider_total_pool_tracks_topups_and_claims() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);

    let user_one = Address::generate(&env);
    let user_two = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = create_token(&env);
    let token = token::Client::new(&env, &token_address);
    let token_admin = token::StellarAssetClient::new(&env, &token_address);

    token_admin.mint(&user_one, &10_000);
    token_admin.mint(&user_two, &10_000);

    client.set_tax_rate(&0);

    let meter_one = client.register_meter(&user_one, &provider, &10, &token_address, &device_key(&env, 1));
    let meter_two = client.register_meter(&user_two, &provider, &10, &token_address, &device_key(&env, 2));

    client.top_up(&meter_one, &5_000);
    client.top_up(&meter_two, &5_000);

    // Verify initial pool
    assert_eq!(client.get_provider_total_pool(&provider), 10_000);

    env.ledger().set_timestamp(100);
    client.claim(&meter_one);

    let window = client.get_provider_window(&provider).unwrap();
    assert!(window.daily_withdrawn > 0);
}

#[test]
fn test_batch_withdraw_all_claims_active_provider_streams() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);

    let user_one = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = create_token(&env);
    let token_admin = token::StellarAssetClient::new(&env, &token_address);

    token_admin.mint(&user_one, &10_000);
    client.set_tax_rate(&500); // 5% tax

    let meter_one = client.register_meter(&user_one, &provider, &10, &token_address, &device_key(&env, 1));
    client.top_up(&meter_one, &5_000);

    env.ledger().set_timestamp(10);
    let result = client.batch_withdraw_all(&provider, &token_address);

    assert_eq!(result.streams_withdrawn, 1);
    assert!(result.total_tax_withheld > 0);
}

// ==================== INTER-PROTOCOL DEBT SERVICE TESTS ====================

#[test]
fn test_service_sorosusu_debt_diverts_funds() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);

    let susu_id = env.register_contract(None, mock_sorosusu::MockSoroSusu);
    let susu_client = mock_sorosusu::MockSoroSusuClient::new(&env, &susu_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = create_token(&env);
    let token_admin = token::StellarAssetClient::new(&env, &token_address);

    token_admin.mint(&user, &100_000);
    client.set_tax_rate(&0);
    client.set_sorosusu_contract(&susu_id);

    let meter_id = client.register_meter(&user, &provider, &10, &token_address, &device_key(&env, 42));
    client.top_up(&meter_id, &100_000);

    // Generate maintenance fund via claim
    env.ledger().set_timestamp(1_000);
    client.claim(&meter_id);

    let fund_before = client.get_maintenance_fund(&meter_id);
    susu_client.set_default(&user, &true);

    client.service_sorosusu_debt(&meter_id);

    let fund_after = client.get_maintenance_fund(&meter_id);
    assert!(fund_after < fund_before);
}

// ==================== PROVIDER RELIABILITY TESTS ====================

#[test]
fn test_reliability_score_logic() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);
    let provider = Address::generate(&env);

    // Report 99 online windows out of 100
    for _ in 0..99u32 {
        client.report_provider_uptime(&provider, &true);
    }
    client.report_provider_uptime(&provider, &false);

    let score = client.get_reliability_score(&provider).unwrap();
    assert_eq!(score.score_bps, 9900);
    assert_eq!(score.badge, ReliabilityBadge::Gold);
}

#[test]
fn test_reliability_score_reset_impact() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_id);
    let provider = Address::generate(&env);

    // Create meter info vector
    let mut meter_infos = Vec::new(&env);
    meter_infos.push_back(MeterInfo {
        user: user1.clone(),
        provider: provider.clone(),
        off_peak_rate: 100,
        token: token_address.clone(),
        billing_type: BillingType::PrePaid,
        device_public_key: device_key1,
    });
    meter_infos.push_back(MeterInfo {
        user: user2.clone(),
        provider: provider.clone(),
        off_peak_rate: 200,
        token: token_address.clone(),
        billing_type: BillingType::PostPaid,
        device_public_key: device_key2,
    });
    meter_infos.push_back(MeterInfo {
        user: user3.clone(),
        provider: provider.clone(),
        off_peak_rate: 150,
        token: token_address.clone(),
        billing_type: BillingType::PrePaid,
        device_public_key: device_key3,
    });

    // Call batch_register_meters
    let batch_event = client.batch_register_meters(&meter_infos);

    // Verify batch event
    assert_eq!(batch_event.start_id, 1);
    assert_eq!(batch_event.end_id, 3);
    assert_eq!(batch_event.count, 3);

    // Verify individual meters were created
    let meter1 = client.get_meter(&1);
    assert!(meter1.is_some());
    let meter1 = meter1.unwrap();
    assert_eq!(meter1.user, user1);
    assert_eq!(meter1.off_peak_rate, 100);
    assert_eq!(meter1.billing_type, BillingType::PrePaid);

    let meter2 = client.get_meter(&2);
    assert!(meter2.is_some());
    let meter2 = meter2.unwrap();
    assert_eq!(meter2.user, user2);
    assert_eq!(meter2.off_peak_rate, 200);
    assert_eq!(meter2.billing_type, BillingType::PostPaid);

    let meter3 = client.get_meter(&3);
    assert!(meter3.is_some());
    let meter3 = meter3.unwrap();
    assert_eq!(meter3.user, user3);
    assert_eq!(meter3.off_peak_rate, 150);
    assert_eq!(meter3.billing_type, BillingType::PrePaid);
}

#[test]
fn test_batch_register_meters_empty_vector() {
    let env = Env::default();
    let contract_address = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_address);

#[test]
fn test_green_energy_bonus() {
    let env = Env::default();
    let contract_address = env.register_contract(None, UtilityContract);
    let client = UtilityContractClient::new(&env, &contract_address);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_address = Address::generate(&env);

    // Register a meter
    let meter_id = client.register_meter_with_mode(
        &user,
        &provider,
        &1000, // off-peak rate
        &token_address,
        &BillingType::PrePaid,
        &BytesN::from_array(&env, &[0; 32]),
    );

    // Set custom green energy discount (10% = 1000 basis points)
    client.set_green_energy_discount(&meter_id, &1000);

    // Top up the meter
    client.top_up(&meter_id, &10000);

    // Mock usage data - renewable energy
    let renewable_usage = SignedUsageData {
        meter_id: meter_id.clone(),
        timestamp: env.ledger().timestamp(),
        watt_hours_consumed: 100,
        units_consumed: 50,
        is_renewable_energy: true,
        signature: BytesN::from_array(&env, &[0; 64]),
        public_key: BytesN::from_array(&env, &[0; 32]),
    };

    // Mock usage data - non-renewable energy
    let non_renewable_usage = SignedUsageData {
        meter_id: meter_id.clone(),
        timestamp: env.ledger().timestamp(),
        watt_hours_consumed: 100,
        units_consumed: 50,
        is_renewable_energy: false,
        signature: BytesN::from_array(&env, &[0; 64]),
        public_key: BytesN::from_array(&env, &[0; 32]),
    };

    // Pair the meter (skip signature verification for test)
    env.storage().instance().set(&DataKey::PairingChallenge(meter_id.clone()), &BytesN::from_array(&env, &[0; 32]));
    let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id.clone())).unwrap();
    meter.is_paired = true;
    env.storage().instance().set(&DataKey::Meter(meter_id.clone()), &meter);

    let initial_balance = meter.balance;

    // Test renewable energy usage (should get 10% discount)
    // Note: In actual test environment, signature verification is skipped
    client.deduct_units(&renewable_usage);

    let renewable_meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id.clone())).unwrap();
    let renewable_cost = initial_balance - renewable_meter.balance;

    // Reset balance for comparison
    renewable_meter.balance = initial_balance;
    env.storage().instance().set(&DataKey::Meter(meter_id.clone()), &renewable_meter);

    // Test non-renewable energy usage (full price)
    client.deduct_units(&non_renewable_usage);

    let final_meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id.clone())).unwrap();
    let non_renewable_cost = initial_balance - final_meter.balance;

    // Verify renewable energy cost is lower (10% discount applied)
    assert!(renewable_cost < non_renewable_cost);

    // Verify renewable energy tracking
    assert!(final_meter.usage_data.renewable_watt_hours > 0);
    assert!(final_meter.usage_data.renewable_percentage > 0);
}

// ============================================================================
// Issue #98: Multi-Sig Provider Withdrawal Requirement Tests
// ============================================================================

#[test]
fn test_configure_multisig_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let provider = Address::generate(&env);

    // Create 5 Finance Department wallets
    let finance_wallet_1 = Address::generate(&env);
    let finance_wallet_2 = Address::generate(&env);
    let finance_wallet_3 = Address::generate(&env);
    let finance_wallet_4 = Address::generate(&env);
    let finance_wallet_5 = Address::generate(&env);

    let mut finance_wallets = Vec::new(&env);
    finance_wallets.push_back(finance_wallet_1.clone());
    finance_wallets.push_back(finance_wallet_2.clone());
    finance_wallets.push_back(finance_wallet_3.clone());
    finance_wallets.push_back(finance_wallet_4.clone());
    finance_wallets.push_back(finance_wallet_5.clone());

    // Configure multi-sig: 3-of-5 required for amounts >= $100,000
    let required_signatures: u32 = 3;
    let threshold_amount: i128 = 100_000_00; // $100,000 in cents

    client.configure_multisig_withdrawal(
        &provider,
        &finance_wallets,
        &required_signatures,
        &threshold_amount,
    );

    // Verify configuration
    let config = client.get_multisig_config(&provider);
    assert_eq!(config.provider, provider);
    assert_eq!(config.finance_wallets.len(), 5);
    assert_eq!(config.required_signatures, 3);
    assert_eq!(config.threshold_amount, threshold_amount);
    assert!(config.is_active);
}

#[test]
fn test_multisig_withdrawal_full_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    // Setup token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Setup users and provider
    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let treasury = Address::generate(&env);

    // Mint tokens
    token_admin_client.mint(&user, &500_000_00); // $500,000 in cents
    token_admin_client.mint(&contract_id, &500_000_00); // Fund contract for withdrawals

    // Create Finance Department wallets
    let finance_wallet_1 = Address::generate(&env);
    let finance_wallet_2 = Address::generate(&env);
    let finance_wallet_3 = Address::generate(&env);
    let finance_wallet_4 = Address::generate(&env);
    let finance_wallet_5 = Address::generate(&env);

    let mut finance_wallets = Vec::new(&env);
    finance_wallets.push_back(finance_wallet_1.clone());
    finance_wallets.push_back(finance_wallet_2.clone());
    finance_wallets.push_back(finance_wallet_3.clone());
    finance_wallets.push_back(finance_wallet_4.clone());
    finance_wallets.push_back(finance_wallet_5.clone());

    // Register meter
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &100, &token_address, &device_public_key, &0);

    // Top up meter
    client.top_up(&meter_id, &300_000_00, &user); // $300,000

    // Configure multi-sig: 3-of-5 for amounts >= $100,000
    client.configure_multisig_withdrawal(
        &provider,
        &finance_wallets,
        &3,
        &100_000_00,
    );

    // Propose a withdrawal of $150,000 (above threshold, requires multi-sig)
    let withdrawal_amount: i128 = 150_000_00;
    let request_id = client.propose_multisig_withdrawal(
        &provider,
        &meter_id,
        &withdrawal_amount,
        &treasury,
    );

    // Verify request was created
    let request = client.get_withdrawal_request(&provider, &request_id);
    assert_eq!(request.amount_usd_cents, withdrawal_amount);
    assert_eq!(request.approval_count, 1); // Proposer auto-approves
    assert!(!request.is_executed);
    assert!(!request.is_cancelled);

    // Second approval
    client.approve_multisig_withdrawal(&provider, &request_id);

    let request_after_2 = client.get_withdrawal_request(&provider, &request_id);
    assert_eq!(request_after_2.approval_count, 2);

    // Third approval (reaches threshold)
    client.approve_multisig_withdrawal(&provider, &request_id);

    let request_after_3 = client.get_withdrawal_request(&provider, &request_id);
    assert_eq!(request_after_3.approval_count, 3);

    // Execute withdrawal
    client.execute_multisig_withdrawal(&provider, &request_id);

    // Verify execution
    let executed_request = client.get_withdrawal_request(&provider, &request_id);
    assert!(executed_request.is_executed);
}

#[test]
fn test_multisig_requires_check() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let provider = Address::generate(&env);

    // Create Finance Department wallets
    let mut finance_wallets = Vec::new(&env);
    for _ in 0..5 {
        finance_wallets.push_back(Address::generate(&env));
    }

    // Configure multi-sig with $100,000 threshold
    client.configure_multisig_withdrawal(
        &provider,
        &finance_wallets,
        &3,
        &100_000_00,
    );

    // Check amounts below threshold don't require multi-sig
    assert!(!client.requires_multisig(&provider, &50_000_00)); // $50,000
    assert!(!client.requires_multisig(&provider, &99_999_99)); // Just below threshold

    // Check amounts at or above threshold require multi-sig
    assert!(client.requires_multisig(&provider, &100_000_00)); // Exactly threshold
    assert!(client.requires_multisig(&provider, &200_000_00)); // Above threshold
    assert!(client.requires_multisig(&provider, &1_000_000_00)); // $1M
}

#[test]
fn test_multisig_revoke_approval() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    // Setup token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let treasury = Address::generate(&env);

    token_admin_client.mint(&user, &500_000_00);
    token_admin_client.mint(&contract_id, &500_000_00);

    // Create Finance Department wallets
    let mut finance_wallets = Vec::new(&env);
    for _ in 0..5 {
        finance_wallets.push_back(Address::generate(&env));
    }

    // Register and fund meter
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &100, &token_address, &device_public_key, &0);
    client.top_up(&meter_id, &300_000_00, &user);

    // Configure multi-sig
    client.configure_multisig_withdrawal(&provider, &finance_wallets, &3, &100_000_00);

    // Propose withdrawal
    let request_id = client.propose_multisig_withdrawal(
        &provider,
        &meter_id,
        &150_000_00,
        &treasury,
    );

    // Add second approval
    client.approve_multisig_withdrawal(&provider, &request_id);

    let request_before = client.get_withdrawal_request(&provider, &request_id);
    assert_eq!(request_before.approval_count, 2);

    // Revoke one approval
    client.revoke_multisig_approval(&provider, &request_id);

    let request_after = client.get_withdrawal_request(&provider, &request_id);
    assert_eq!(request_after.approval_count, 1);
}

#[test]
fn test_multisig_cancel_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    // Setup token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let treasury = Address::generate(&env);

    token_admin_client.mint(&user, &500_000_00);
    token_admin_client.mint(&contract_id, &500_000_00);

    // Create Finance Department wallets
    let mut finance_wallets = Vec::new(&env);
    for _ in 0..5 {
        finance_wallets.push_back(Address::generate(&env));
    }

    // Register and fund meter
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &100, &token_address, &device_public_key, &0);
    client.top_up(&meter_id, &300_000_00, &user);

    // Configure multi-sig
    client.configure_multisig_withdrawal(&provider, &finance_wallets, &3, &100_000_00);

    // Propose withdrawal
    let request_id = client.propose_multisig_withdrawal(
        &provider,
        &meter_id,
        &150_000_00,
        &treasury,
    );

    // Verify request is active
    let request_before = client.get_withdrawal_request(&provider, &request_id);
    assert!(!request_before.is_cancelled);

    // Cancel withdrawal
    client.cancel_multisig_withdrawal(&provider, &request_id);

    // Verify cancellation
    let request_after = client.get_withdrawal_request(&provider, &request_id);
    assert!(request_after.is_cancelled);
}
test.rs
#[test]
fn test_multisig_enable_disable() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let provider = Address::generate(&env);

    // Create Finance Department wallets
    let mut finance_wallets = Vec::new(&env);
    for _ in 0..5 {
        finance_wallets.push_back(Address::generate(&env));
    }

    // Configure multi-sig
    client.configure_multisig_withdrawal(&provider, &finance_wallets, &3, &100_000_00);

    // Verify it's active
    let config = client.get_multisig_config(&provider);
    assert!(config.is_active);

    // Disable multi-sig
    client.disable_multisig(&provider);

    // Verify disabled
    let config_disabled = client.get_multisig_config(&provider);
    assert!(!config_disabled.is_active);

    // Re-enable multi-sig
    client.enable_multisig(&provider);

    // Verify re-enabled
    let config_enabled = client.get_multisig_config(&provider);
    assert!(config_enabled.is_active);
}

#[test]
fn test_multisig_update_config() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let provider = Address::generate(&env);

    // Create initial Finance Department wallets
    let mut finance_wallets = Vec::new(&env);
    for _ in 0..5 {
        finance_wallets.push_back(Address::generate(&env));
    }

    // Configure multi-sig with initial values
    client.configure_multisig_withdrawal(&provider, &finance_wallets, &3, &100_000_00);

    // Create new Finance Department wallets
    let mut new_finance_wallets = Vec::new(&env);
    for _ in 0..4 {
        new_finance_wallets.push_back(Address::generate(&env));
    }

    // Update configuration with new values
    client.update_multisig_config(
        &provider,
        &new_finance_wallets,
        &2, // Now 2-of-4
        &50_000_00, // Lower threshold: $50,000
    );

    // Verify updated config
    let config = client.get_multisig_config(&provider);
    assert_eq!(config.finance_wallets.len(), 4);
    assert_eq!(config.required_signatures, 2);
    assert_eq!(config.threshold_amount, 50_000_00);
}

#[test]
fn test_multisig_get_withdrawal_request_count() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    // Setup token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let treasury = Address::generate(&env);

    token_admin_client.mint(&user, &1_000_000_00);
    token_admin_client.mint(&contract_id, &1_000_000_00);

    // Create Finance Department wallets
    let mut finance_wallets = Vec::new(&env);
    for _ in 0..5 {
        finance_wallets.push_back(Address::generate(&env));
    }

    // Register and fund meter
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &100, &token_address, &device_public_key, &0);
    client.top_up(&meter_id, &500_000_00, &user);

    // Configure multi-sig
    client.configure_multisig_withdrawal(&provider, &finance_wallets, &3, &100_000_00);

    // Initial count should be 0
    assert_eq!(client.get_withdrawal_request_count(&provider), 0);

    // Create first request
    client.propose_multisig_withdrawal(&provider, &meter_id, &150_000_00, &treasury);
    assert_eq!(client.get_withdrawal_request_count(&provider), 1);

    // Create second request
    client.propose_multisig_withdrawal(&provider, &meter_id, &200_000_00, &treasury);
    assert_eq!(client.get_withdrawal_request_count(&provider), 2);

    // Create third request
    client.propose_multisig_withdrawal(&provider, &meter_id, &100_000_00, &treasury);
    assert_eq!(client.get_withdrawal_request_count(&provider), 3);
}

#[test]
fn test_multisig_has_approved_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    // Setup token
    let token_admin = Address::generate(&env);
    let token_address = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let treasury = Address::generate(&env);

    token_admin_client.mint(&user, &500_000_00);
    token_admin_client.mint(&contract_id, &500_000_00);

    // Create Finance Department wallets
    let finance_wallet_1 = Address::generate(&env);
    let finance_wallet_2 = Address::generate(&env);
    let finance_wallet_3 = Address::generate(&env);
    let finance_wallet_4 = Address::generate(&env);
    let finance_wallet_5 = Address::generate(&env);

    let mut finance_wallets = Vec::new(&env);
    finance_wallets.push_back(finance_wallet_1.clone());
    finance_wallets.push_back(finance_wallet_2.clone());
    finance_wallets.push_back(finance_wallet_3.clone());
    finance_wallets.push_back(finance_wallet_4.clone());
    finance_wallets.push_back(finance_wallet_5.clone());

    // Register and fund meter
    let device_public_key = BytesN::from_array(&env, &[1u8; 32]);
    let meter_id = client.register_meter(&user, &provider, &100, &token_address, &device_public_key, &0);
    client.top_up(&meter_id, &300_000_00, &user);

    // Configure multi-sig
    client.configure_multisig_withdrawal(&provider, &finance_wallets, &3, &100_000_00);

    // Propose withdrawal (proposer auto-approves)
    let request_id = client.propose_multisig_withdrawal(
        &provider,
        &meter_id,
        &150_000_00,
        &treasury,
    );

    // Check approval status - proposer (first wallet) should have approved
    assert!(client.has_approved_withdrawal(&provider, &request_id, &finance_wallet_1));

    // Other wallets should not have approved yet
    assert!(!client.has_approved_withdrawal(&provider, &request_id, &finance_wallet_2));
    assert!(!client.has_approved_withdrawal(&provider, &request_id, &finance_wallet_3));
}
