//! Auto-upgrade module for viberails.
//!
//! Handles downloading and installing new versions of the binary with the following
//! security measures:
//! - Proper file locking using `flock()` to prevent concurrent upgrades
//! - PID verification for stale lock detection
//! - Required checksum verification (configurable)
//! - Atomic binary replacement using `rename()`
//! - Rollback mechanism on failure
//! - Randomized upgrade binary path to prevent pre-placement attacks
//! - HOME environment validation

use std::env;

use anyhow::Result;
use log::{info, warn};

mod config;
mod poll;

pub use config::UpgradeConfig;

use poll::{
    DEF_UPGRADE_CHECK, UpgradeLock, previous_upgrade_cleanup, self_upgrade_with_force,
    spawn_upgrade_with_force,
};

use crate::{common::PROJECT_VERSION, hooks::binary_location};

/// Upgrade result for user-facing output.
pub enum UpgradeResult {
    /// Already on the latest version
    AlreadyLatest { version: String },
    /// Successfully upgraded to a new version
    Upgraded { from: String, to: String },
    /// Force reinstalled the same version
    Reinstalled { version: String },
    /// Upgrade spawned in background (Windows self-upgrade)
    Spawned,
    /// Another upgrade is already in progress
    InProgress,
}

////////////////////////////////////////////////////////////////////////////////
// PUBLIC API
////////////////////////////////////////////////////////////////////////////////

/// Checks if an upgrade should be performed and triggers it if needed.
///
/// Called on program exit. Acquires the upgrade lock first, then checks if at
/// least `DEF_UPGRADE_CHECK` (15 minutes) has elapsed since the last poll.
/// This avoids a TOCTOU race where multiple concurrent processes could all
/// see `should_poll()` = true and redundantly hit the network.
///
/// Parameters: None
///
/// Returns: `Ok(())` on success or if no upgrade needed, Err on upgrade failure
pub fn poll_upgrade() -> Result<()> {
    let force_upgrade = env::var("VB_FORCE_UPGRADE").is_ok();

    // Acquire the upgrade lock before checking poll state to prevent
    // concurrent processes from all deciding to poll at the same time.
    previous_upgrade_cleanup();

    let Some(_lock) = UpgradeLock::acquire()? else {
        // Another upgrade is already in progress
        return Ok(());
    };

    let mut upgrade_config = UpgradeConfig::load();

    if upgrade_config.should_poll(&DEF_UPGRADE_CHECK) || force_upgrade {
        info!("time to upgrade");
        // Record that we polled, even if the upgrade itself fails or finds
        // we are already on the latest version. This prevents hammering the
        // server on repeated short-lived invocations.
        if let Err(e) = upgrade_config.record_poll() {
            warn!("unable to save upgrade poll state: {e}");
        }
        // Auto-upgrade: never forces reinstall, not verbose (background operation)
        upgrade_locked(false, false)?;
    }

    Ok(())
}

/// Performs the upgrade process.
///
/// Acquires exclusive lock, determines if we need to spawn a helper process
/// (for self-upgrade on Windows), and performs the actual upgrade.
///
/// Parameters:
///   - `force`: If true, skip version check and always download/install
///   - `verbose`: If true, print progress messages to stdout
///
/// Returns: `UpgradeResult` indicating what happened, Err on failure
pub fn upgrade(force: bool, verbose: bool) -> Result<UpgradeResult> {
    info!("Upgrading (force={force}, verbose={verbose})");

    previous_upgrade_cleanup();

    // Acquire upgrade lock to prevent concurrent upgrades
    let Some(_lock) = UpgradeLock::acquire()? else {
        // Another upgrade is already in progress
        return Ok(UpgradeResult::InProgress);
    };

    upgrade_locked(force, verbose)
}

/// Performs the upgrade process assuming the caller already holds the
/// [`UpgradeLock`].
///
/// Parameters:
///   - `force`: If true, skip version check and always download/install
///   - `verbose`: If true, print progress messages to stdout
///
/// Returns: `UpgradeResult` indicating what happened, Err on failure
fn upgrade_locked(force: bool, verbose: bool) -> Result<UpgradeResult> {
    let bin_location = binary_location()?;
    let bin_current = env::current_exe()?;

    if bin_location == bin_current {
        // We can't upgrade ourselves, spawn from temporary location
        info!("spawning upgrade process");
        if verbose {
            println!("Current version: {PROJECT_VERSION}");
            println!("Spawning upgrade process in background...");
        }
        spawn_upgrade_with_force(force)?;
        Ok(UpgradeResult::Spawned)
    } else {
        self_upgrade_with_force(force, verbose)
    }
}

#[cfg(test)]
mod tests;
