use soroban_sdk::{Env, Address, Symbol};
use std::collections::HashMap;

#[derive(Clone)]
pub struct MasterStream {
    pub account: Address,
    pub sensors: HashMap<String, i128>, // MAC address → latest consumption payload
    pub balance: i128,
}

pub fn add_sensor(env: &Env, account: Address, mac: String) {
    let mut stream: MasterStream = env.storage().get(&format!("stream:{}", account))
        .unwrap_or(MasterStream { account: account.clone(), sensors: HashMap::new(), balance: 0 });

    stream.sensors.insert(mac.clone(), 0);
    env.storage().set(&format!("stream:{}", account), &stream);

    env.events().publish(
        (Symbol::short("SensorAdded"),),
        (account, mac),
    );
}

pub fn remove_sensor(env: &Env, account: Address, mac: String) {
    let mut stream: MasterStream = env.storage().get(&format!("stream:{}", account))
        .unwrap_or_else(|| panic!("Stream not found"));

    stream.sensors.remove(&mac);
    env.storage().set(&format!("stream:{}", account), &stream);

    env.events().publish(
        (Symbol::short("SensorRemoved"),),
        (account, mac),
    );
}

pub fn record_consumption(env: &Env, account: Address, mac: String, payload: i128) {
    let mut stream: MasterStream = env.storage().get(&format!("stream:{}", account))
        .unwrap_or_else(|| panic!("Stream not found"));

    if !stream.sensors.contains_key(&mac) {
        panic!("Sensor not registered");
    }

    stream.sensors.insert(mac.clone(), payload);

    // Aggregate total
    let total: i128 = stream.sensors.values().sum();

    // Deduct from balance
    stream.balance -= total;
    env.storage().set(&format!("stream:{}", account), &stream);

    env.events().publish(
        (Symbol::short("AggregateUpdated"),),
        (account, total, stream.balance),
    );
}

pub fn validate_invariants(env: &Env, account: Address) {
    let stream: MasterStream = env.storage().get(&format!("stream:{}", account))
        .unwrap_or_else(|| panic!("Stream not found"));

    if stream.balance < 0 {
        panic!("Balance invariant violated");
    }

    if stream.sensors.len() > 10 {
        panic!("Too many sensors linked");
    }
}

