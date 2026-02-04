use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, Result, anyhow, bail};
use colored::Colorize;
use log::{error, info, warn};

use crate::{
    common::{EXECUTABLE_NAME, display_authorize_help, print_header, project_config_dir},
    config::Config,
    providers::{ProviderRegistry, select_providers, select_providers_for_uninstall},
    tui::{MessageStyle, message_prompt},
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

fn uninstall_config() -> Result<()> {
    let config_dir = project_config_dir()?;

    if !config_dir.exists() {
        warn!("Config directory {} doesn't exist", config_dir.display());
        return Ok(());
    }

    fs::remove_dir_all(&config_dir)
        .with_context(|| format!("Unable to delete config directory {}", config_dir.display()))?;

    info!("Config directory {} was deleted", config_dir.display());

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

    message_prompt(
        " Installation Complete ",
        "Hooks installed successfully! Your AI coding tools will now use viberails.",
        MessageStyle::Success,
    )?;

    Ok(())
}

/// Uninstall hooks only, keeping the binary installed.
///
/// This allows users to remove hooks from selected providers while
/// keeping the binary available for future use.
pub fn uninstall_hooks() -> Result<()> {
    //
    // Create the registry and let the user select providers to uninstall from
    //
    let registry = ProviderRegistry::new();

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

    println!("\nHooks removed. Binary retained for future use.");

    Ok(())
}

/// Fully uninstall: remove hooks and delete the binary.
///
/// This performs a complete uninstallation by removing hooks from
/// selected providers and deleting the binary if all hooks are removed.
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
    let all_uninstalled =
        should_delete_binary(selection.selected_ids.len(), installed_count_before);

    if all_uninstalled {
        if let Err(e) = uninstall_binary(&dst) {
            error!("Unable to delete binary ({e}");
            success = false;
        }
        if let Err(e) = uninstall_config() {
            error!("Unable to delete config ({e}");
            success = false;
        }
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
///   - `selected_count`: Number of providers selected for uninstall
///   - `installed_count_before`: Number of providers with hooks installed BEFORE uninstall
///
/// Returns: true if binary should be deleted, false otherwise
#[must_use]
pub fn should_delete_binary(selected_count: usize, installed_count_before: usize) -> bool {
    selected_count >= installed_count_before
}
