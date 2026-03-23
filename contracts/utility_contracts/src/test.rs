#![cfg(test)]

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

#[test]
fn test_claim_within_daily_limit_tracks_withdrawn() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);

    // Setup a token
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Initial funding
    token_admin_client.mint(&user, &1000);

    // 1. Register Meter
    let rate = 10; // 10 tokens per second
    let meter_id = client.register_meter(&user, &provider, &rate, &token_address);
    assert_eq!(meter_id, 1);

    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.rate_per_second, 10);
    assert_eq!(meter.balance, 0);
    assert_eq!(meter.is_active, false);

    // 2. Top up
    client.top_up(&meter_id, &500);
    let meter = client.get_meter(&meter_id).unwrap();
    assert_eq!(meter.balance, 500);
    assert_eq!(meter.is_active, true);
    assert_eq!(token.balance(&user), 500);
    assert_eq!(token.balance(&contract_id), 500);

    // 3. Claim within the 10% daily limit
    env.ledger().set_timestamp(env.ledger().timestamp() + 5);
    client.claim(&meter_id);

    let meter = client.get_meter(&meter_id).unwrap();
    let provider_window = client.get_provider_window(&provider).unwrap();

    assert_eq!(meter.balance, 450);
    assert_eq!(token.balance(&provider), 50);
    assert_eq!(token.balance(&contract_id), 450);
    assert_eq!(provider_window.daily_withdrawn, 50);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_claim_reverts_when_daily_limit_is_exceeded() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1000);

    let meter_id = client.register_meter(&user, &provider, &10, &token_address);
    client.top_up(&meter_id, &500);

    env.ledger().set_timestamp(env.ledger().timestamp() + 10);
    client.claim(&meter_id);
}

#[test]
fn test_daily_limit_resets_after_24_hours() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token = token::Client::new(&env, &token_address);
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user, &1_000_000);

    let meter_id = client.register_meter(&user, &provider, &1, &token_address);
    client.top_up(&meter_id, &1_000_000);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 10_000);
    client.claim(&meter_id);

    let provider_window = client.get_provider_window(&provider).unwrap();
    assert_eq!(provider_window.daily_withdrawn, 10_000);

    env.ledger()
        .set_timestamp(env.ledger().timestamp() + (24 * 60 * 60) + 5_000);
    client.claim(&meter_id);

    let provider_window = client.get_provider_window(&provider).unwrap();
    assert_eq!(provider_window.daily_withdrawn, 91_400);
    assert_eq!(token.balance(&provider), 101_400);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_daily_limit_is_shared_across_provider_meters() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(UtilityContract, ());
    let client = UtilityContractClient::new(&env, &contract_id);

    let user_one = Address::generate(&env);
    let user_two = Address::generate(&env);
    let provider = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token_address = env.register_stellar_asset_contract(token_admin.clone());
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    token_admin_client.mint(&user_one, &500);
    token_admin_client.mint(&user_two, &500);

    let meter_one = client.register_meter(&user_one, &provider, &10, &token_address);
    let meter_two = client.register_meter(&user_two, &provider, &10, &token_address);

    client.top_up(&meter_one, &500);
    client.top_up(&meter_two, &500);

    env.ledger().set_timestamp(env.ledger().timestamp() + 5);
    client.claim(&meter_one);
    client.claim(&meter_two);

    env.ledger().set_timestamp(env.ledger().timestamp() + 1);
    client.claim(&meter_one);
}
