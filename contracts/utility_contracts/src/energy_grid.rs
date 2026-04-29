use soroban_sdk::{Env, Address, Symbol};

#[derive(Clone)]
pub struct LoadConfig {
    pub peak_load_multiplier: i128,
    pub low_load_discount: i128,
    pub active_window: String, // "peak" or "offpeak"
}

pub fn set_peak_multiplier(env: &Env, admin: Address, multiplier: i128) {
    // Only grid admin can configure
    let stored_admin: Address = env.storage().get("grid_admin").unwrap();
    if admin != stored_admin {
        panic!("Unauthorized");
    }
    env.storage().set("peak_load_multiplier", &multiplier);
    env.events().publish(
        (Symbol::short("MultiplierActivated"),),
        ("peak", multiplier),
    );
}

pub fn set_low_discount(env: &Env, admin: Address, discount: i128) {
    let stored_admin: Address = env.storage().get("grid_admin").unwrap();
    if admin != stored_admin {
        panic!("Unauthorized");
    }
    env.storage().set("low_load_discount", &discount);
    env.events().publish(
        (Symbol::short("MultiplierActivated"),),
        ("offpeak", discount),
    );
}

pub fn bill_consumption(env: &Env, user: Address, base_rate: i128, timestamp: u64) -> i128 {
    // Determine active window based on time-of-use
    let hour = (timestamp / 3600) % 24;
    let mut final_rate = base_rate;

    if hour >= 18 && hour <= 22 {
        // Peak hours
        let peak: i128 = env.storage().get("peak_load_multiplier").unwrap_or(2);
        final_rate *= peak;
        env.events().publish(
            (Symbol::short("BillingApplied"),),
            (user.clone(), "peak", final_rate),
        );
    } else {
        // Off-peak hours
        let discount: i128 = env.storage().get("low_load_discount").unwrap_or(1);
        final_rate = final_rate * discount / 100; // discount as percentage
        env.events().publish(
            (Symbol::short("BillingApplied"),),
            (user.clone(), "offpeak", final_rate),
        );
    }

    // Debit user balance
    let mut balance: i128 = env.storage().get(&format!("balance:{}", user)).unwrap_or(0);
    balance -= final_rate;
    env.storage().set(&format!("balance:{}", user), &balance);

    final_rate
}
