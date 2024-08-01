#![cfg(test)]

use std::sync::Arc;

use crate::api_util::AppStateData;
use crate::clickhouse::ClickHouseConnectionInfo;
use crate::config_parser::Config;

pub fn get_unit_test_app_state_data(
    config: Config,
    clickhouse_healthy: Option<bool>,
) -> AppStateData {
    let http_client = reqwest::Client::new();
    let clickhouse_connection_info = ClickHouseConnectionInfo::new("", true, clickhouse_healthy)
        .map_err(|e| eprintln!("Failed to create ClickHouse connection info: {e}"))
        .unwrap();

    AppStateData {
        config: Arc::new(config),
        http_client,
        clickhouse_connection_info,
    }
}
