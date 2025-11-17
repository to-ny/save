use crate::config::LoadTestConfig;
use crate::transactions::*;
use goose::prelude::*;
use std::sync::Arc;

pub fn build_scenario(config: &LoadTestConfig) -> Scenario {
    let weights = &config.scenarios.read_heavy;
    let shared_keys = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let config_clone = config.clone();

    scenario!("ReadHeavy")
        .register_transaction(
            Transaction::new(Arc::new(setup_user(config_clone, shared_keys))).set_name("Setup"),
        )
        .register_transaction(
            transaction!(put_object)
                .set_name("PUT")
                .set_weight(weights.put_weight)
                .unwrap(),
        )
        .register_transaction(
            transaction!(get_object)
                .set_name("GET")
                .set_weight(weights.get_weight)
                .unwrap(),
        )
        .register_transaction(
            transaction!(delete_object)
                .set_name("DELETE")
                .set_weight(weights.delete_weight)
                .unwrap(),
        )
        .register_transaction(
            transaction!(list_objects)
                .set_name("LIST")
                .set_weight(weights.list_weight)
                .unwrap(),
        )
}
