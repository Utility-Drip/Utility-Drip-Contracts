#![no_std]
use soroban_sdk::{contract, contracttype, contractimpl, Address, Env, token, Symbol};

#[contracttype]
#[derive(Clone)]
pub struct Meter {
    pub user: Address,
    pub provider: Address,
    pub rate_per_unit: i128,
    pub balance: i128,
    pub last_update: u64,
    pub is_active: bool,
    pub token: Address,
    pub max_flow_rate_per_hour: i128,
    pub last_claim_time: u64,
    pub claimed_this_hour: i128,
    pub heartbeat: u64,
}

#[contracttype]
pub enum DataKey {
    Meter(u64),
    Count,
    Oracle,
}

#[contract]
pub struct UtilityContract;

#[contractimpl]
impl UtilityContract {
    pub fn set_oracle(env: Env, oracle: Address) {
        env.storage().instance().set(&DataKey::Oracle, &oracle);
    }

    pub fn register_meter(
        env: Env,
        user: Address,
        provider: Address,
        rate: i128,
        token: Address,
    ) -> u64 {
        user.require_auth();
        let mut count: u64 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);
        count += 1;

        let meter = Meter {
            user,
            provider,
            rate_per_unit: rate,
            balance: 0,
            last_update: env.ledger().timestamp(),
            is_active: false,
            token,
            max_flow_rate_per_hour: rate * 3600, // Default to 1 hour of normal flow
            last_claim_time: env.ledger().timestamp(),
            claimed_this_hour: 0,
            heartbeat: env.ledger().timestamp(),
        };

        env.storage().instance().set(&DataKey::Meter(count), &meter);
        env.storage().instance().set(&DataKey::Count, &count);
        count
    }

    pub fn top_up(env: Env, meter_id: u64, amount: i128) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.user.require_auth();

        let client = token::Client::new(&env, &meter.token);
        client.transfer(&meter.user, &env.current_contract_address(), &amount);

        meter.balance += amount;
        meter.is_active = true;
        meter.last_update = env.ledger().timestamp();
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn deduct_units(env: Env, meter_id: u64, units_consumed: i128) {
        let oracle: Address = env.storage().instance().get(&DataKey::Oracle).expect("Oracle address not set");
        oracle.require_auth();

        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        
        let cost = units_consumed * meter.rate_per_unit;
        
        if cost > 0 {
            let actual_claim = if cost > meter.balance {
                meter.balance
            } else {
                cost
            };

            if actual_claim > 0 {
                let client = token::Client::new(&env, &meter.token);
                client.transfer(&env.current_contract_address(), &meter.provider, &actual_claim);
                meter.balance -= actual_claim;
            }
        }

        meter.last_update = env.ledger().timestamp();
        if meter.balance <= 0 {
            meter.is_active = false;
        }

        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);

        // Emit UsageReported event
        env.events().publish(
            (Symbol::new(&env, "UsageReported"), meter_id),
            (units_consumed, cost)
        );
    }

    pub fn get_meter(env: Env, meter_id: u64) -> Option<Meter> {
        env.storage().instance().get(&DataKey::Meter(meter_id))
    }

    pub fn calculate_expected_depletion(env: Env, meter_id: u64) -> Option<u64> {
        if let Some(meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
            if meter.balance <= 0 || meter.rate_per_second <= 0 {
                return Some(0); // Already depleted or no consumption
            }
            
            let seconds_until_depletion = meter.balance / meter.rate_per_second;
            let current_time = env.ledger().timestamp();
            Some(current_time + seconds_until_depletion as u64)
        } else {
            None
        }
    }

    pub fn emergency_shutdown(env: Env, meter_id: u64) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.provider.require_auth();
        
        // Immediately disable the meter
        meter.is_active = false;
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn update_heartbeat(env: Env, meter_id: u64) {
        let mut meter: Meter = env.storage().instance().get(&DataKey::Meter(meter_id)).ok_or("Meter not found").unwrap();
        meter.user.require_auth();
        
        meter.heartbeat = env.ledger().timestamp();
        
        env.storage().instance().set(&DataKey::Meter(meter_id), &meter);
    }

    pub fn is_meter_offline(env: Env, meter_id: u64) -> bool {
        if let Some(meter) = env.storage().instance().get::<_, Meter>(&DataKey::Meter(meter_id)) {
            let current_time = env.ledger().timestamp();
            let time_since_heartbeat = current_time.checked_sub(meter.heartbeat).unwrap_or(0);
            // Consider offline if heartbeat is > 1 hour old (3600 seconds)
            time_since_heartbeat > 3600
        } else {
            true // Meter not found, consider offline
        }
    }
}

mod test;
