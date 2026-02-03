use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, Result, anyhow, bail};
use colored::Colorize;
use log::{error, info, warn};

use crate::{
    common::{EXECUTABLE_NAME, display_authorize_help, print_header},
    config::Config,
    providers::{ProviderRegistry, select_providers, select_providers_for_uninstall},
};

const LABEL_WIDTH: usize = 20;

struct InstallResult {
    provider_name: String,
    hooktype: &'static str,
    result: Result<(), Error>,
}

impl fmt::Display for InstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.result {
            Ok(()) => write!(
                f,
                "{:<LABEL_WIDTH$} {:<20} {}",
                self.provider_name,
                self.hooktype,
                "[SUCCESS]".green()
            ),
            Err(e) => write!(
                f,
                "{:<LABEL_WIDTH$} {:<20} {} {}",
                self.provider_name,
                self.hooktype,
                "[FAILURE]".red(),
                e
            ),
        }
    }
}

fn install_hooks_for_provider(
    registry: &ProviderRegistry,
    provider_id: &str,
) -> Vec<InstallResult> {
    let mut results = vec![];

    let Some(factory) = registry.get(provider_id) else {
        results.push(InstallResult {
            provider_name: provider_id.to_string(),
            hooktype: "*",
            result: Err(anyhow!("Unknown provider")),
        });
        return results;
    };

    let provider = match factory.create() {
        Ok(p) => p,
        Err(e) => {
            results.push(InstallResult {
                provider_name: factory.display_name().to_string(),
                hooktype: "*",
                result: Err(e),
            });
            return results;
        }
    };

    for hook_type in factory.supported_hooks() {
        let ret = provider.install(hook_type);

        results.push(InstallResult {
            provider_name: factory.display_name().to_string(),
            hooktype: hook_type,
            result: ret,
        });
    }

    results
}

fn uninstall_hooks_for_provider(
    registry: &ProviderRegistry,
    provider_id: &str,
) -> Vec<InstallResult> {
    let mut results = vec![];

    let Some(factory) = registry.get(provider_id) else {
        return results;
    };

    let provider = match factory.create() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to create provider {provider_id}: {e}");
            return results;
        }
    };

    for hook_type in factory.supported_hooks() {
        let ret = provider.uninstall(hook_type);

        results.push(InstallResult {
            provider_name: factory.display_name().to_string(),
            hooktype: hook_type,
            result: ret,
        });
    }

    results
}

fn display_results(results: &[InstallResult]) {
    print_header();
    for r in results {
        println!("{r}");
    }
}

fn uninstall_binary(dst: &Path) -> Result<()> {
    info!("Uninstall location {}", dst.display());

    if !dst.exists() {
        warn!("{} doesn't exist", dst.display());
        return Ok(());
    }

    fs::remove_file(dst).with_context(|| format!("Unable to delete {}", dst.display()))?;

    info!("{} was deleted", dst.display());

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// PUBLIC
////////////////////////////////////////////////////////////////////////////////

pub fn binary_location() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| {
        anyhow!("Unable to determine home directory. Ensure HOME environment variable is set")
    })?;

    let local_bin = home.join(".local").join("bin");

    if !local_bin.exists() {
        fs::create_dir_all(&local_bin)
            .with_context(|| format!("Unable to create {}", local_bin.display()))?;
    }

    Ok(local_bin.join(EXECUTABLE_NAME))
}

pub fn install_binary(dst: &Path) -> Result<()> {
    info!("Install location {}", dst.display());

    let current_exe = env::current_exe().context("Unable to find current exe")?;

    //
    // Skip copy if we're already running from the install location
    //
    if current_exe == dst {
        info!("already installed at {}", dst.display());
        return Ok(());
    }

    //
    // On Linux, we can't overwrite a running binary ("Text file busy" error).
    // However, we can delete it first - Linux allows deleting a running executable
    // because the file is only truly removed when all processes using it exit.
    //
    if dst.exists() {
        fs::remove_file(dst).with_context(|| format!("Unable to remove {}", dst.display()))?;
        info!("removed existing binary at {}", dst.display());
    }

    fs::copy(&current_exe, dst).with_context(|| {
        format!(
            "Unable to copy {} to {}",
            current_exe.display(),
            dst.display()
        )
    })?;

    info!("copied to {}", dst.display());

    Ok(())
}

pub fn install() -> Result<()> {
    //
    // Make sure we're autorized, otherwise it'll fail silently
    //
    let config = Config::load()?;
    if !config.org.authorized() {
        display_authorize_help();
        bail!("Not Authorized");
    }

    //
    // Create the registry and let the user select providers
    //
    let registry = ProviderRegistry::new();
    let selection = select_providers(&registry)?;

    let Some(selection) = selection else {
        println!("Installation cancelled.");
        return Ok(());
    };

    if selection.selected_ids.is_empty() {
        println!("No providers selected.");
        return Ok(());
    }

    //
    // We also have to install ourselves on the host. We'll do like claude-code
    // and install ourselves in ~/.local/bin/
    //
    // It doesn't matter if it's not in the path since the hook contains
    // an absolute path
    //
    let dst = binary_location()?;
    install_binary(&dst)?;

    //
    // Install hooks for each selected provider
    //
    let mut all_results = Vec::new();

    for provider_id in &selection.selected_ids {
        let results = install_hooks_for_provider(&registry, provider_id);
        all_results.extend(results);
    }

    display_results(&all_results);

    Ok(())
}

pub fn uninstall() -> Result<()> {
    let mut success = true;
    let dst = binary_location()?;

    //
    // Create the registry and let the user select providers to uninstall from
    //
    let registry = ProviderRegistry::new();

    // Count installed hooks BEFORE uninstalling (to correctly detect partial uninstall)
    // Bug fix: Previously this was checked AFTER uninstalling, so count was always 0
    let installed_count_before = registry
        .discover_all_with_hooks_check()
        .iter()
        .filter(|d| d.hooks_installed)
        .count();

    let selection = select_providers_for_uninstall(&registry)?;

    let Some(selection) = selection else {
        println!("Uninstallation cancelled.");
        return Ok(());
    };

    if selection.selected_ids.is_empty() {
        println!("No providers selected.");
        return Ok(());
    }

    //
    // Uninstall hooks for each selected provider
    //
    let mut all_results = Vec::new();

    for provider_id in &selection.selected_ids {
        let results = uninstall_hooks_for_provider(&registry, provider_id);
        all_results.extend(results);
    }

    display_results(&all_results);

    //
    // Only delete binary if ALL providers with hooks installed were uninstalled
    //
    let all_uninstalled = should_delete_binary(selection.selected_ids.len(), installed_count_before);

    if all_uninstalled {
        if let Err(e) = uninstall_binary(&dst) {
            error!("Unable to delete binary ({e}");
            success = false;
        }
        // Note: We intentionally preserve the config (including team membership)
        // so users can reinstall without needing to rejoin their team.
    } else {
        println!(
            "\nHooks removed from selected tools. Binary and config retained for remaining tools."
        );
    }

    if success {
        Ok(())
    } else {
        Err(anyhow!("Uninstall failures. See logs"))
    }
}

/// Determine if the binary should be deleted based on uninstall selection.
/// Returns true only if ALL providers with hooks installed were selected for uninstall.
///
/// Parameters:
///   - selected_count: Number of providers selected for uninstall
///   - installed_count_before: Number of providers with hooks installed BEFORE uninstall
///
/// Returns: true if binary should be deleted, false otherwise
#[must_use]
pub fn should_delete_binary(selected_count: usize, installed_count_before: usize) -> bool {
    selected_count >= installed_count_before
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // should_delete_binary tests
    // =========================================================================

    #[test]
    fn test_should_delete_binary_all_uninstalled() {
        // All 2 installed providers were selected for uninstall
        assert!(should_delete_binary(2, 2));
    }

    #[test]
    fn test_should_delete_binary_more_selected_than_installed() {
        // Edge case: selected more than installed (shouldn't happen but handle gracefully)
        assert!(should_delete_binary(3, 2));
    }

    #[test]
    fn test_should_delete_binary_partial_uninstall() {
        // Only 1 of 2 installed providers selected - should NOT delete binary
        assert!(!should_delete_binary(1, 2));
    }

    #[test]
    fn test_should_delete_binary_no_selection() {
        // No providers selected for uninstall
        assert!(!should_delete_binary(0, 2));
    }

    #[test]
    fn test_should_delete_binary_single_provider() {
        // Only 1 provider installed and it was selected
        assert!(should_delete_binary(1, 1));
    }

    #[test]
    fn test_should_delete_binary_no_providers_installed() {
        // Edge case: no providers were installed (shouldn't reach uninstall but handle it)
        // 0 selected >= 0 installed = true
        assert!(should_delete_binary(0, 0));
    }

    #[test]
    fn test_should_delete_binary_many_providers_partial() {
        // 5 providers installed, only 3 selected for uninstall
        assert!(!should_delete_binary(3, 5));
    }

    #[test]
    fn test_should_delete_binary_many_providers_all() {
        // 5 providers installed, all 5 selected for uninstall
        assert!(should_delete_binary(5, 5));
    }

    // =========================================================================
    // Bug regression tests
    // =========================================================================

    #[test]
    fn test_bug_regression_count_before_not_after() {
        // This test documents the bug that was fixed:
        // Previously, installed_count was checked AFTER uninstall, so it would be 0
        // making all_uninstalled always true.
        //
        // With the fix, we capture installed_count BEFORE uninstall.
        // Example scenario:
        // - 2 providers installed (Claude Code, Codex)
        // - User selects only 1 for uninstall (Codex)
        // - installed_count_before = 2
        // - selected_count = 1
        // - should_delete_binary(1, 2) = false (binary preserved)
        //
        // BUG behavior (checking after):
        // - After uninstalling Codex, only Claude Code has hooks
        // - installed_count_after = 1 (but actually 0 in buggy code due to stale check)
        // - should_delete_binary(1, 0) = true (WRONG - binary deleted!)

        // Correct behavior: partial uninstall should NOT delete binary
        let installed_before = 2;
        let selected_for_uninstall = 1;
        assert!(
            !should_delete_binary(selected_for_uninstall, installed_before),
            "Partial uninstall should NOT delete binary"
        );

        // Full uninstall SHOULD delete binary
        let selected_all = 2;
        assert!(
            should_delete_binary(selected_all, installed_before),
            "Full uninstall should delete binary"
        );
    }
}
