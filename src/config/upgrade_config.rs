use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use log::debug;
use serde::{Deserialize, Serialize};

use crate::common::project_data_dir;

const UPGRADE_STATE_FILE: &str = "upgrade_state.json";

/// Persistent state for upgrade polling, stored in the data directory.
///
/// Tracks when the last poll and last successful upgrade occurred so we
/// only hit the network once per `DEF_UPGRADE_CHECK` interval instead of
/// every time the binary happens to be older than that threshold.
#[derive(Default, Serialize, Deserialize)]
pub struct UpgradeConfig {
    /// Unix timestamp (seconds) of the last time we polled for a new version.
    #[serde(default)]
    pub last_poll: u64,
    /// Unix timestamp (seconds) of the last successful upgrade.
    #[serde(default)]
    pub last_upgrade: u64,
}

impl UpgradeConfig {
    /// Returns the path to the upgrade state file.
    fn state_file_path() -> Result<PathBuf> {
        let data_dir = project_data_dir()?;
        Ok(data_dir.join(UPGRADE_STATE_FILE))
    }

    /// Loads the upgrade state from disk. Returns default if file is missing or corrupt.
    #[must_use]
    pub fn load() -> Self {
        let Ok(path) = Self::state_file_path() else {
            return Self::default();
        };

        Self::load_from_path(&path)
    }

    /// Loads from a specific path. Returns default if file is missing or corrupt.
    pub(crate) fn load_from_path(path: &Path) -> Self {
        let Ok(content) = fs::read_to_string(path) else {
            return Self::default();
        };

        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Saves the upgrade state to disk.
    fn save(&self) -> Result<()> {
        let path = Self::state_file_path()?;
        self.save_to_path(&path)
    }

    /// Saves to a specific path.
    pub(crate) fn save_to_path(&self, path: &Path) -> Result<()> {
        let content =
            serde_json::to_string_pretty(self).context("Unable to serialize upgrade state")?;

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;

            let mut fd = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("Unable to write {}", path.display()))?;

            fd.write_all(content.as_bytes())
                .with_context(|| format!("Failed to write upgrade state to {}", path.display()))?;
        }

        #[cfg(not(unix))]
        fs::write(path, content).with_context(|| format!("Unable to write {}", path.display()))?;

        debug!("Upgrade state saved to {}", path.display());
        Ok(())
    }

    /// Returns true if enough time has elapsed since the last poll.
    #[must_use]
    pub fn should_poll(&self, interval: &Duration) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        now.saturating_sub(self.last_poll) >= interval.as_secs()
    }

    /// Records that a poll just happened and persists to disk.
    pub fn record_poll(&mut self) -> Result<()> {
        self.last_poll = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.save()
    }

    /// Records that an upgrade just succeeded and persists to disk.
    pub fn record_upgrade(&mut self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_poll = now;
        self.last_upgrade = now;
        self.save()
    }
}
