use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::common::project_config_dir;

const DEF_UPGRADE_CHECK: Duration = Duration::from_mins(15);
const UPGRADE_CONFIG_FILE: &str = "upgrade.json";

#[derive(Serialize, Deserialize, Default)]
struct UpgradeConfig {
    #[serde(default)]
    last_upgrade_ts: u64,
}

fn upgrade_config_path() -> Option<PathBuf> {
    project_config_dir()
        .ok()
        .map(|d| d.join(UPGRADE_CONFIG_FILE))
}

fn load_upgrade_config() -> UpgradeConfig {
    let Some(path) = upgrade_config_path() else {
        return UpgradeConfig::default();
    };

    fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn save_upgrade_config(config: &UpgradeConfig) {
    let Some(path) = upgrade_config_path() else {
        return;
    };

    if let Ok(data) = serde_json::to_string_pretty(config) {
        let _ = fs::write(&path, data);
    }
}

pub fn touch_last_upgrade_check() {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut config = load_upgrade_config();
    config.last_upgrade_ts = ts;
    save_upgrade_config(&config);
}

#[must_use]
pub fn is_upgrade_check_due() -> bool {
    let config = load_upgrade_config();

    if config.last_upgrade_ts == 0 {
        return true;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let elapsed = Duration::from_secs(now.saturating_sub(config.last_upgrade_ts));

    elapsed > DEF_UPGRADE_CHECK
}
