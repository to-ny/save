use crate::config::LoadTestConfig;
use crate::transactions::*;
use goose::prelude::*;

pub fn build_scenario(config: &LoadTestConfig) -> Scenario {
    let weights = &config.scenarios.mixed;

    scenario!("Mixed")
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
