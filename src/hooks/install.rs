use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, Result, anyhow, bail};
use colored::Colorize;
use log::{error, info, warn};

/// Result of a safe directory removal attempt.
#[derive(Debug, PartialEq)]
enum SafeRemoveResult {
    /// Directory was successfully removed
    Removed,
    /// Directory didn't exist (not an error)
    NotFound,
    /// Path exists but is not a directory (e.g. a regular file)
    NotADirectory,
    /// Path is a symlink - refused to remove for safety
    SymlinkRefused,
}

use crate::{
    common::{
        EXECUTABLE_NAME, PROJECT_NAME, display_authorize_help, print_header,
        project_config_dir_path, project_data_dir_path, validated_binary_dir,
    },
    config::Config,
    providers::{ProviderRegistry, select_providers, select_providers_for_uninstall},
    tui::{MessageStyle, message_prompt},
};

/// Parse provider selection from CLI argument.
/// Supports comma-separated provider IDs or "all".
/// For install: "all" means all detected providers.
/// For uninstall: "all" means all providers with hooks installed.
///
/// Parameters:
///   - `registry`: The provider registry to look up providers
///   - `provider_list`: Comma-separated provider IDs or "all"
///   - `is_uninstall`: Whether this is for uninstall (affects "all" behavior)
///
/// Returns: List of selected provider IDs
fn parse_provider_selection(
    registry: &ProviderRegistry,
    provider_list: &str,
    is_uninstall: bool,
) -> Result<Vec<&'static str>> {
    let discoveries = if is_uninstall {
        registry.discover_all_with_hooks_check()
    } else {
        registry.discover_all()
    };

    if provider_list.trim().eq_ignore_ascii_case("all") {
        // Select all appropriate providers based on mode
        let selected: Vec<&'static str> = discoveries
            .iter()
            .filter(|d| if is_uninstall { d.hooks_installed } else { d.detected })
            .map(|d| d.id)
            .collect();

        if selected.is_empty() {
            if is_uninstall {
                bail!("No providers have hooks installed");
            }
            bail!("No supported AI coding tools detected");
        }

        Ok(selected)
    } else {
        // Parse comma-separated list
        let requested_ids: Vec<&str> = provider_list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        if requested_ids.is_empty() {
            bail!("No provider IDs specified");
        }

        let mut selected = Vec::new();
        for requested_id in requested_ids {
            // Find the provider in discoveries
            if let Some(discovery) = discoveries.iter().find(|d| d.id == requested_id) {
                selected.push(discovery.id);
            } else {
                bail!("Unknown provider ID: {requested_id}");
            }
        }

        Ok(selected)
    }
}

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

/// Check hook removal results for failures.
///
/// Returns `true` if all hook operations succeeded, `false` if any provider
/// returned empty results (provider creation failed) or any individual hook
/// removal returned an error.
///
/// Parameters:
///   - `results_per_provider`: Slice of (`provider_id`, results) pairs from each provider
///
/// Returns: `true` if all results are non-empty and successful, `false` otherwise
fn check_hook_results(results_per_provider: &[(&str, &[InstallResult])]) -> bool {
    for (provider_id, results) in results_per_provider {
        // Empty results means provider creation failed silently
        if results.is_empty() {
            warn!("Provider {provider_id} returned no results — creation may have failed");
            return false;
        }

        // Check for individual hook removal failures
        for result in *results {
            if result.result.is_err() {
                return false;
            }
        }
    }

    true
}

/// Safely removes a file, refusing to follow symlinks.
///
/// This prevents symlink attacks where a malicious symlink could trick
/// the uninstaller into deleting files outside our control.
///
/// Parameters:
///   - `path`: Path to the file to remove
///
/// Returns: `Ok(())` on success, Err on failure
fn safe_remove_file(path: &Path) -> Result<()> {
    info!("Safe remove file: {}", path.display());

    // Use symlink_metadata to check the path itself, not the target
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                warn!(
                    "Refusing to remove symlink at {} - this could be an attack",
                    path.display()
                );
                bail!(
                    "Path {} is a symlink. Refusing to remove for safety.",
                    path.display()
                );
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("{} doesn't exist", path.display());
            return Ok(());
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Unable to stat {}", path.display()));
        }
    }

    fs::remove_file(path).with_context(|| format!("Unable to delete {}", path.display()))?;

    info!("{} was deleted", path.display());

    Ok(())
}

/// Safely removes a directory and all its contents, refusing to follow symlinks.
///
/// This function checks that the target path is not a symlink before removal.
/// The standard `fs::remove_dir_all` on most platforms does NOT follow symlinks
/// when removing directory contents, but we add an explicit check on the top-level
/// path for defense in depth.
///
/// Parameters:
///   - `path`: Path to the directory to remove
///
/// Returns: `SafeRemoveResult` indicating what happened
fn safe_remove_dir_all(path: &Path) -> Result<SafeRemoveResult> {
    info!("Safe remove directory: {}", path.display());

    // Use symlink_metadata to check the path itself, not the target
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                warn!(
                    "Refusing to remove symlink at {} - this could be an attack",
                    path.display()
                );
                return Ok(SafeRemoveResult::SymlinkRefused);
            }
            if !metadata.is_dir() {
                warn!("{} exists but is not a directory", path.display());
                return Ok(SafeRemoveResult::NotADirectory);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("Directory {} doesn't exist", path.display());
            return Ok(SafeRemoveResult::NotFound);
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Unable to stat {}", path.display()));
        }
    }

    fs::remove_dir_all(path)
        .with_context(|| format!("Unable to delete directory {}", path.display()))?;

    info!("Directory {} was deleted", path.display());

    Ok(SafeRemoveResult::Removed)
}

/// Checks whether `path` refers to the currently running executable.
///
/// Compares canonicalized paths so that symlinks, relative segments, etc.
/// are resolved before comparison. Returns `false` for any path that
/// cannot be resolved (e.g. it doesn't exist).
///
/// Parameters:
///   - `path`: Path to compare against the running binary
///
/// Returns: `true` if `path` is the same file as the running process
fn is_current_executable(path: &Path) -> bool {
    let Ok(current_exe) = env::current_exe() else {
        return false;
    };
    let Ok(canonical_current) = current_exe.canonicalize() else {
        return false;
    };
    let Ok(canonical_path) = path.canonicalize() else {
        return false;
    };
    canonical_current == canonical_path
}

/// Removes the installed binary, using `self-replace` when deleting the
/// currently running executable (required on Windows where a running .exe
/// cannot be deleted via normal `fs::remove_file`).
///
/// Safety checks preserved from `safe_remove_file`:
///   - Refuses to remove symlinks (prevents symlink-based attacks)
///   - Gracefully handles already-missing files
///
/// Parameters:
///   - `dst`: Path to the binary to remove
///
/// Returns: `Ok(())` on success, Err on failure
fn uninstall_binary(dst: &Path) -> Result<()> {
    info!("Uninstall location {}", dst.display());

    // Check the path itself (not the target) — refuse symlinks for safety
    match fs::symlink_metadata(dst) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                warn!(
                    "Refusing to remove symlink at {} - this could be an attack",
                    dst.display()
                );
                bail!(
                    "Path {} is a symlink. Refusing to remove for safety.",
                    dst.display()
                );
            }
        }
        // File already gone — nothing to do
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("{} doesn't exist", dst.display());
            return Ok(());
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Unable to stat {}", dst.display()));
        }
    }

    if is_current_executable(dst) {
        // On Windows the running .exe is locked; self_delete_at moves it to a
        // temp location with FILE_FLAG_DELETE_ON_CLOSE, freeing the original
        // path immediately. On Unix it's equivalent to unlink().
        info!("Deleting self (running executable) at {}", dst.display());
        self_replace::self_delete_at(dst)
            .with_context(|| format!("Unable to self-delete {}", dst.display()))?;
    } else {
        fs::remove_file(dst)
            .with_context(|| format!("Unable to delete {}", dst.display()))?;
    }

    info!("{} was deleted", dst.display());
    Ok(())
}

/// Removes the config directory without recreating it first.
///
/// Uses `project_config_dir_path()` to resolve the path (no directory creation),
/// then delegates to `safe_remove_dir_all`.
///
/// Parameters: None
///
/// Returns: `Ok(())` on success, Err on failure
fn uninstall_config() -> Result<()> {
    let config_dir = project_config_dir_path()?;

    match safe_remove_dir_all(&config_dir)? {
        SafeRemoveResult::Removed => {
            info!("Config directory {} was deleted", config_dir.display());
        }
        SafeRemoveResult::NotFound => {
            // Already logged in safe_remove_dir_all
        }
        SafeRemoveResult::NotADirectory => {
            warn!(
                "Config path {} is not a directory, skipping removal",
                config_dir.display()
            );
        }
        SafeRemoveResult::SymlinkRefused => {
            bail!(
                "Config directory {} is a symlink. Refusing to remove for safety.",
                config_dir.display()
            );
        }
    }

    Ok(())
}

/// Removes the data directory containing debug logs, upgrade state, etc.
///
/// Uses `project_data_dir_path()` to resolve the path (no directory creation),
/// then delegates to `safe_remove_dir_all`.
///
/// Parameters: None
///
/// Returns: `Ok(())` on success, Err on failure
fn uninstall_data_dir() -> Result<()> {
    let data_dir = project_data_dir_path()?;

    match safe_remove_dir_all(&data_dir)? {
        SafeRemoveResult::Removed => {
            info!("Data directory {} was deleted", data_dir.display());
        }
        SafeRemoveResult::NotFound => {
            // Already logged in safe_remove_dir_all
        }
        SafeRemoveResult::NotADirectory => {
            warn!(
                "Data path {} is not a directory, skipping removal",
                data_dir.display()
            );
        }
        SafeRemoveResult::SymlinkRefused => {
            bail!(
                "Data directory {} is a symlink. Refusing to remove for safety.",
                data_dir.display()
            );
        }
    }

    Ok(())
}

/// Cleans up upgrade-related files from the binary directory.
///
/// This removes:
/// - Upgrade lock file (`.viberails.upgrade.lock`)
/// - Temporary upgrade binaries (`viberails_upgrade_*`)
/// - Temporary new binaries (`.viberails_new_*`)
///
/// This function refuses to remove symlinks for safety.
///
/// Parameters:
///   - `bin_dir`: Path to the binary directory
///
/// Returns: Number of files cleaned up
#[allow(clippy::arithmetic_side_effects)]
fn cleanup_upgrade_files(bin_dir: &Path) -> usize {
    let mut cleaned = 0;

    // Patterns for files to clean up
    let lock_file = bin_dir.join(".viberails.upgrade.lock");
    let upgrade_prefix = format!("{PROJECT_NAME}_upgrade_");
    let new_binary_prefix = format!(".{PROJECT_NAME}_new_");

    // Pre-check with symlink_metadata (not exists() which follows symlinks).
    // The actual safety check is inside safe_remove_file — this is only
    // for counting purposes.
    let lock_present = fs::symlink_metadata(&lock_file).is_ok();
    if lock_present && safe_remove_file(&lock_file).is_ok() {
        cleaned += 1;
    }

    // Remove upgrade and temp binaries
    if let Ok(entries) = fs::read_dir(bin_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            if name.starts_with(&upgrade_prefix) || name.starts_with(&new_binary_prefix) {
                let path = entry.path();
                // Check if it's a symlink - refuse to remove if so
                if let Ok(metadata) = fs::symlink_metadata(&path)
                    && metadata.file_type().is_symlink()
                {
                    warn!(
                        "Refusing to remove symlink {}: potential attack",
                        path.display()
                    );
                    continue;
                }
                if fs::remove_file(&path).is_ok() {
                    info!("Removed temp file: {}", path.display());
                    cleaned += 1;
                }
            }
        }
    }

    cleaned
}

////////////////////////////////////////////////////////////////////////////////
// PUBLIC
////////////////////////////////////////////////////////////////////////////////

/// Returns the installation path for the binary.
///
/// Uses ~/.local/bin/ on Unix-like systems. Validates the home directory
/// to prevent HOME environment injection attacks.
///
/// Parameters: None
///
/// Returns: Path to the binary installation location
pub fn binary_location() -> Result<PathBuf> {
    Ok(validated_binary_dir()?.join(EXECUTABLE_NAME))
}

/// Copies the current executable to the installation location.
///
/// Parameters:
///   - `dst`: Destination path for the binary
///
/// Returns: `Ok(())` on success, Err on failure
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

/// Install hooks for selected providers. Supports both interactive and non-interactive modes.
///
/// Parameters:
///   - `providers`: Optional comma-separated provider IDs or "all" for non-interactive mode.
///     None for interactive TUI selection.
///
/// Returns: `Ok(())` on success, Err on failure
pub fn install(providers: Option<&str>) -> Result<()> {
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

    let selected_ids = if let Some(provider_list) = providers {
        // Non-interactive mode: parse provider IDs from CLI
        parse_provider_selection(&registry, provider_list, false)?
    } else {
        // Interactive mode: show selection UI
        let selection = select_providers(&registry)?;

        let Some(selection) = selection else {
            println!("Installation cancelled.");
            return Ok(());
        };

        if selection.selected_ids.is_empty() {
            println!("No providers selected.");
            return Ok(());
        }

        selection.selected_ids
    };

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

    for provider_id in &selected_ids {
        let results = install_hooks_for_provider(&registry, provider_id);
        all_results.extend(results);
    }

    display_results(&all_results);

    // Only show interactive prompt if in interactive mode
    if providers.is_none() {
        message_prompt(
            " Installation Complete ",
            "Hooks installed successfully! Your AI coding tools will now use viberails.",
            MessageStyle::Success,
        )?;
    } else {
        println!("\nHooks installed successfully! Your AI coding tools will now use viberails.");
    }

    Ok(())
}

/// Uninstall hooks only, keeping the binary installed.
///
/// This allows users to remove hooks from selected providers while
/// keeping the binary available for future use.
///
/// Parameters: None
///
/// Returns: `Ok(())` on success, Err on failure
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

/// Uninstall everything: remove all hooks from all providers, delete the binary, and remove all data.
///
/// This performs a complete cleanup without prompting for provider selection.
/// All detected providers with hooks installed will have their hooks removed.
/// Also removes:
/// - Binary from `~/.local/bin/`
/// - Config directory (`~/.config/viberails/`)
/// - Data directory (`~/.local/share/viberails/`) containing debug logs and upgrade state
/// - Upgrade lock files and temporary binaries
///
/// Parameters: None
///
/// Returns: `Ok(())` on success, Err on failure
pub fn uninstall_all() -> Result<()> {
    let mut success = true;

    // Resolve the binary location — failure here should NOT block the rest
    // of cleanup (hooks, config, data). The user may be running uninstall-all
    // on a system where HOME is unset or bin dir can't be determined.
    let binary_info = binary_location().and_then(|dst| {
        let bin_dir = dst
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("Binary path has no parent directory: {}", dst.display()))?;
        Ok((dst, bin_dir))
    });

    let registry = ProviderRegistry::new();

    // Discover all providers and uninstall hooks from those that have them installed
    let discoveries = registry.discover_all_with_hooks_check();
    let providers_with_hooks: Vec<_> = discoveries
        .iter()
        .filter(|d| d.hooks_installed)
        .collect();

    if providers_with_hooks.is_empty() {
        println!("No hooks are currently installed.");
    } else {
        // Uninstall hooks for all providers that have them installed.
        // Collect per-provider results so check_hook_results can detect
        // both empty-result providers (creation failure) and individual errors.
        let mut per_provider: Vec<(&str, Vec<InstallResult>)> = Vec::new();

        for discovery in &providers_with_hooks {
            let results = uninstall_hooks_for_provider(&registry, discovery.id);
            per_provider.push((discovery.id, results));
        }

        // Build refs for the check helper
        let refs: Vec<(&str, &[InstallResult])> = per_provider
            .iter()
            .map(|(id, r)| (*id, r.as_slice()))
            .collect();

        if !check_hook_results(&refs) {
            success = false;
        }

        // Flatten for display
        let all_results: Vec<_> = per_provider
            .into_iter()
            .flat_map(|(_, results)| results)
            .collect();

        display_results(&all_results);
    }

    // Binary-dependent cleanup: upgrade files and binary removal
    match &binary_info {
        Ok((dst, bin_dir)) => {
            // Clean up upgrade-related files (lock files, temp binaries)
            let cleaned_files = cleanup_upgrade_files(bin_dir);
            if cleaned_files > 0 {
                println!("\nCleaned up {cleaned_files} temporary file(s).");
            }

            // Delete the binary
            if let Err(e) = uninstall_binary(dst) {
                error!("Unable to delete binary: {e}");
                success = false;
            } else {
                println!("Binary removed: {}", dst.display());
            }
        }
        Err(e) => {
            error!("Unable to determine binary location: {e}");
            error!("Skipping binary and upgrade file cleanup, continuing with config/data removal.");
            success = false;
        }
    }

    // Delete config directory (independent of binary location)
    if let Err(e) = uninstall_config() {
        error!("Unable to delete config: {e}");
        success = false;
    } else {
        println!("Configuration removed.");
    }

    // Delete data directory (independent of binary location)
    if let Err(e) = uninstall_data_dir() {
        error!("Unable to delete data directory: {e}");
        success = false;
    } else {
        println!("Data directory removed (debug logs, upgrade state).");
    }

    if success {
        println!("\n{}", "Full cleanup complete.".green());
        Ok(())
    } else {
        Err(anyhow!("Uninstall had some failures. See logs for details."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // -------------------------------------------------------------------------
    // safe_remove_file tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_safe_remove_file_removes_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "content").unwrap();

        assert!(file_path.exists());
        safe_remove_file(&file_path).unwrap();
        assert!(!file_path.exists());
    }

    #[test]
    fn test_safe_remove_file_handles_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("nonexistent.txt");

        // Should succeed gracefully for missing files
        assert!(safe_remove_file(&file_path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_safe_remove_file_refuses_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let symlink = dir.path().join("symlink.txt");

        fs::write(&target, "precious data").unwrap();
        std::os::unix::fs::symlink(&target, &symlink).unwrap();

        // Should refuse to remove the symlink
        let result = safe_remove_file(&symlink);
        assert!(result.is_err());

        // Target file should still exist
        assert!(target.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "precious data");
    }

    // -------------------------------------------------------------------------
    // safe_remove_dir_all tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_safe_remove_dir_all_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("subdir");
        fs::create_dir_all(target.join("nested")).unwrap();
        fs::write(target.join("nested/file.txt"), "data").unwrap();

        let result = safe_remove_dir_all(&target).unwrap();
        assert!(matches!(result, SafeRemoveResult::Removed));
        assert!(!target.exists());
    }

    #[test]
    fn test_safe_remove_dir_all_handles_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nonexistent");

        let result = safe_remove_dir_all(&target).unwrap();
        assert!(matches!(result, SafeRemoveResult::NotFound));
    }

    #[test]
    fn test_safe_remove_dir_all_returns_not_a_directory_for_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("not_a_dir.txt");
        fs::write(&file_path, "content").unwrap();

        // A regular file is not a directory — should return NotADirectory
        let result = safe_remove_dir_all(&file_path).unwrap();
        assert_eq!(result, SafeRemoveResult::NotADirectory);

        // File should still exist (we didn't remove it)
        assert!(file_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_safe_remove_dir_all_refuses_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real_dir");
        let symlink = dir.path().join("symlink_dir");

        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("important.txt"), "precious").unwrap();
        std::os::unix::fs::symlink(&target, &symlink).unwrap();

        // Should refuse to remove the symlink
        let result = safe_remove_dir_all(&symlink).unwrap();
        assert!(matches!(result, SafeRemoveResult::SymlinkRefused));

        // Target directory and contents should still exist
        assert!(target.exists());
        assert_eq!(
            fs::read_to_string(target.join("important.txt")).unwrap(),
            "precious"
        );
    }

    // -------------------------------------------------------------------------
    // cleanup_upgrade_files tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_cleanup_upgrade_files_removes_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path();

        // Create matching files
        fs::write(bin_dir.join(".viberails.upgrade.lock"), "123").unwrap();
        fs::write(bin_dir.join("viberails_upgrade_abc123"), "").unwrap();
        fs::write(bin_dir.join("viberails_upgrade_def456"), "").unwrap();
        fs::write(bin_dir.join(".viberails_new_xyz789"), "").unwrap();

        let cleaned = cleanup_upgrade_files(bin_dir);

        assert_eq!(cleaned, 4);
        assert!(!bin_dir.join(".viberails.upgrade.lock").exists());
        assert!(!bin_dir.join("viberails_upgrade_abc123").exists());
        assert!(!bin_dir.join("viberails_upgrade_def456").exists());
        assert!(!bin_dir.join(".viberails_new_xyz789").exists());
    }

    #[test]
    fn test_cleanup_upgrade_files_ignores_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path();

        // Create unrelated files that should NOT be removed
        fs::write(bin_dir.join("other_upgrade_abc"), "").unwrap();
        fs::write(bin_dir.join("viberails_config_backup"), "").unwrap();
        fs::write(bin_dir.join("random_file.txt"), "").unwrap();

        let cleaned = cleanup_upgrade_files(bin_dir);

        assert_eq!(cleaned, 0);
        assert!(bin_dir.join("other_upgrade_abc").exists());
        assert!(bin_dir.join("viberails_config_backup").exists());
        assert!(bin_dir.join("random_file.txt").exists());
    }

    #[test]
    fn test_cleanup_upgrade_files_returns_zero_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();

        let cleaned = cleanup_upgrade_files(dir.path());
        assert_eq!(cleaned, 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_cleanup_upgrade_files_skips_symlink_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path();

        // Create a target file that should NOT be deleted
        let target = bin_dir.join("important_binary");
        fs::write(&target, "precious").unwrap();

        // Create a symlink disguised as a temp upgrade file
        std::os::unix::fs::symlink(&target, bin_dir.join("viberails_upgrade_malicious")).unwrap();

        let cleaned = cleanup_upgrade_files(bin_dir);

        // Symlink should be skipped, not counted as cleaned
        assert_eq!(cleaned, 0);

        // Target file should still exist with original content
        assert!(target.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "precious");
    }

    #[test]
    fn test_cleanup_upgrade_files_mixed_matching_and_unrelated() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path();

        // Matching files
        fs::write(bin_dir.join("viberails_upgrade_abc"), "").unwrap();
        fs::write(bin_dir.join(".viberails.upgrade.lock"), "123").unwrap();

        // Unrelated files
        fs::write(bin_dir.join("keep_this.txt"), "keep").unwrap();
        fs::write(bin_dir.join("viberails"), "binary").unwrap();

        let cleaned = cleanup_upgrade_files(bin_dir);

        assert_eq!(cleaned, 2);
        assert!(!bin_dir.join("viberails_upgrade_abc").exists());
        assert!(!bin_dir.join(".viberails.upgrade.lock").exists());
        assert!(bin_dir.join("keep_this.txt").exists());
        assert!(bin_dir.join("viberails").exists());
    }

    // -------------------------------------------------------------------------
    // is_current_executable tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_is_current_executable_with_current_exe() {
        // The test runner binary is the current executable
        let current = env::current_exe().unwrap();
        assert!(is_current_executable(&current));
    }

    #[test]
    fn test_is_current_executable_with_different_path() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("some_other_binary");
        fs::write(&other, "not the running binary").unwrap();

        assert!(!is_current_executable(&other));
    }

    #[test]
    fn test_is_current_executable_with_nonexistent_path() {
        let path = Path::new("/tmp/definitely_does_not_exist_12345");
        assert!(!is_current_executable(path));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_current_executable_resolves_symlinks() {
        // Create a symlink pointing at the current test binary
        let current = env::current_exe().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("symlinked_exe");
        std::os::unix::fs::symlink(&current, &link).unwrap();

        // canonicalize resolves the symlink, so both should match
        assert!(is_current_executable(&link));
    }

    // -------------------------------------------------------------------------
    // uninstall_binary tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_uninstall_binary_removes_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test_binary");
        fs::write(&file_path, "binary content").unwrap();

        assert!(file_path.exists());
        uninstall_binary(&file_path).unwrap();
        assert!(!file_path.exists());
    }

    #[test]
    fn test_uninstall_binary_handles_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("nonexistent_binary");

        // Should succeed gracefully when file doesn't exist
        assert!(uninstall_binary(&file_path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_uninstall_binary_refuses_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real_binary");
        let symlink = dir.path().join("symlink_binary");

        fs::write(&target, "precious binary").unwrap();
        std::os::unix::fs::symlink(&target, &symlink).unwrap();

        // Should refuse to remove the symlink
        let result = uninstall_binary(&symlink);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("symlink"),
            "Error should mention symlink: {err_msg}"
        );

        // Target file should be untouched
        assert!(target.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "precious binary");
    }

    #[cfg(unix)]
    #[test]
    fn test_uninstall_binary_refuses_symlink_even_when_target_missing() {
        let dir = tempfile::tempdir().unwrap();
        let symlink = dir.path().join("dangling_symlink");

        // Create a dangling symlink (target doesn't exist)
        std::os::unix::fs::symlink("/nonexistent/target", &symlink).unwrap();

        // symlink_metadata sees the symlink itself, so this should be refused
        let result = uninstall_binary(&symlink);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("symlink"),
            "Error should mention symlink: {err_msg}"
        );
    }

    #[test]
    fn test_uninstall_binary_removes_readonly_file() {
        // On Unix, removing a file only requires write permission on the
        // parent directory, not the file itself. This verifies that
        // read-only binaries are still removed properly.
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("readonly_binary");
        fs::write(&file_path, "binary").unwrap();

        // Make the file read-only
        let mut perms = fs::metadata(&file_path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file_path, perms).unwrap();

        // Should still succeed (parent dir is writable)
        uninstall_binary(&file_path).unwrap();
        assert!(!file_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_uninstall_binary_refuses_symlink_chain() {
        // Symlink chain: link_a -> link_b -> real_file
        // Should be refused because the top-level path IS a symlink.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real_binary");
        let link_b = dir.path().join("link_b");
        let link_a = dir.path().join("link_a");

        fs::write(&target, "precious data").unwrap();
        std::os::unix::fs::symlink(&target, &link_b).unwrap();
        std::os::unix::fs::symlink(&link_b, &link_a).unwrap();

        let result = uninstall_binary(&link_a);
        assert!(result.is_err());

        // Entire chain and target must be untouched
        assert!(target.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "precious data");
    }

    #[test]
    fn test_uninstall_binary_with_path_containing_spaces() {
        // Verify paths with spaces don't cause issues
        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("path with spaces");
        fs::create_dir_all(&subdir).unwrap();
        let file_path = subdir.join("my binary");
        fs::write(&file_path, "content").unwrap();

        uninstall_binary(&file_path).unwrap();
        assert!(!file_path.exists());
    }

    #[test]
    fn test_uninstall_binary_with_unicode_path() {
        // Verify unicode paths work correctly
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("binário_日本語");
        fs::write(&file_path, "content").unwrap();

        uninstall_binary(&file_path).unwrap();
        assert!(!file_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_is_current_executable_with_symlink_chain() {
        // Symlink chain: link_a -> link_b -> current_exe
        // canonicalize resolves the full chain, so should still match.
        let current = env::current_exe().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let link_b = dir.path().join("link_b");
        let link_a = dir.path().join("link_a");

        std::os::unix::fs::symlink(&current, &link_b).unwrap();
        std::os::unix::fs::symlink(&link_b, &link_a).unwrap();

        assert!(is_current_executable(&link_a));
    }

    #[test]
    fn test_is_current_executable_with_empty_file() {
        // A zero-byte file is clearly not the running binary
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        fs::write(&empty, "").unwrap();

        assert!(!is_current_executable(&empty));
    }

    // -------------------------------------------------------------------------
    // SafeRemoveResult::NotADirectory tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_safe_remove_dir_all_not_a_directory_leaves_file_intact() {
        // When safe_remove_dir_all encounters a regular file, it should
        // return NotADirectory and leave the file untouched.
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("regular_file.dat");
        fs::write(&file_path, "important data").unwrap();

        let result = safe_remove_dir_all(&file_path).unwrap();
        assert_eq!(result, SafeRemoveResult::NotADirectory);

        // File must still exist with original content
        assert!(file_path.exists());
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "important data");
    }

    #[test]
    fn test_safe_remove_dir_all_empty_dir_returns_removed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("empty_dir");
        fs::create_dir(&target).unwrap();

        let result = safe_remove_dir_all(&target).unwrap();
        assert_eq!(result, SafeRemoveResult::Removed);
        assert!(!target.exists());
    }

    // -------------------------------------------------------------------------
    // cleanup_upgrade_files: symlink_metadata pre-check tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_cleanup_upgrade_files_does_not_count_missing_lock_file() {
        // When the lock file doesn't exist, cleaned count should be 0
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path();
        // No lock file created — just an unrelated file
        fs::write(bin_dir.join("unrelated.txt"), "data").unwrap();

        let cleaned = cleanup_upgrade_files(bin_dir);
        assert_eq!(cleaned, 0);
    }

    #[cfg(unix)]
    #[test]
    fn test_cleanup_upgrade_files_refuses_symlink_lock_file() {
        // If the lock file is a symlink, safe_remove_file should refuse it
        // and it should NOT be counted as cleaned.
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path();

        let target = bin_dir.join("real_lock_target");
        fs::write(&target, "lock data").unwrap();
        let lock_file = bin_dir.join(".viberails.upgrade.lock");
        std::os::unix::fs::symlink(&target, &lock_file).unwrap();

        let cleaned = cleanup_upgrade_files(bin_dir);
        // Symlink lock file should be refused, not counted
        assert_eq!(cleaned, 0);
        // Target should be untouched
        assert!(target.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "lock data");
    }

    // -------------------------------------------------------------------------
    // uninstall_config / uninstall_data_dir path-only resolution tests
    //
    // These tests mutate process-global env vars. All env-var-mutating tests
    // hold ENV_TEST_MUTEX to prevent races under parallel test execution.
    // -------------------------------------------------------------------------

    #[test]
    fn test_uninstall_config_no_create_and_remove() {
        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();

        // Sub-test 1: uninstall_config does NOT create the dir when absent
        let config_dir = dir.path().join("config_absent").join("viberails");

        // SAFETY: env mutation serialized by ENV_TEST_MUTEX
        unsafe { std::env::set_var("VIBERAILS_CONFIG_DIR", config_dir.as_os_str()) };

        assert!(!config_dir.exists());
        let result = uninstall_config();
        assert!(result.is_ok());
        assert!(!config_dir.exists(), "uninstall_config must NOT create dir");

        // Sub-test 2: uninstall_config removes an existing dir
        let config_dir2 = dir.path().join("config_present").join("viberails");
        fs::create_dir_all(&config_dir2).unwrap();
        fs::write(config_dir2.join("config.json"), "{}").unwrap();

        unsafe { std::env::set_var("VIBERAILS_CONFIG_DIR", config_dir2.as_os_str()) };

        let result = uninstall_config();
        assert!(result.is_ok());
        assert!(!config_dir2.exists(), "uninstall_config should remove existing dir");

        unsafe { std::env::remove_var("VIBERAILS_CONFIG_DIR") };
    }

    #[test]
    fn test_uninstall_data_dir_no_create_and_remove() {
        let _lock = crate::common::ENV_TEST_MUTEX.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();

        // Sub-test 1: uninstall_data_dir does NOT create the dir when absent
        let data_dir = dir.path().join("data_absent").join("viberails");

        // SAFETY: env mutation serialized by ENV_TEST_MUTEX
        unsafe { std::env::set_var("VIBERAILS_DATA_DIR", data_dir.as_os_str()) };

        assert!(!data_dir.exists());
        let result = uninstall_data_dir();
        assert!(result.is_ok());
        assert!(!data_dir.exists(), "uninstall_data_dir must NOT create dir");

        // Sub-test 2: uninstall_data_dir removes an existing dir
        let data_dir2 = dir.path().join("data_present").join("viberails");
        fs::create_dir_all(&data_dir2).unwrap();
        fs::write(data_dir2.join("debug.log"), "log data").unwrap();

        unsafe { std::env::set_var("VIBERAILS_DATA_DIR", data_dir2.as_os_str()) };

        let result = uninstall_data_dir();
        assert!(result.is_ok());
        assert!(!data_dir2.exists(), "uninstall_data_dir should remove existing dir");

        unsafe { std::env::remove_var("VIBERAILS_DATA_DIR") };
    }

    // -------------------------------------------------------------------------
    // check_hook_results tests
    //
    // Validates that the extracted helper correctly detects:
    // - all-success results
    // - empty provider results (provider creation failure)
    // - individual hook removal errors
    // -------------------------------------------------------------------------

    #[test]
    fn test_check_hook_results_all_success() {
        let results = vec![
            InstallResult {
                provider_name: "Claude Code".to_string(),
                hooktype: "pre-tool-use",
                result: Ok(()),
            },
            InstallResult {
                provider_name: "Claude Code".to_string(),
                hooktype: "post-tool-use",
                result: Ok(()),
            },
        ];

        let per_provider = vec![("claude-code", results.as_slice())];
        assert!(check_hook_results(&per_provider));
    }

    #[test]
    fn test_check_hook_results_empty_results_flags_failure() {
        // Empty results means provider creation failed silently
        let empty: Vec<InstallResult> = vec![];
        let per_provider = vec![("broken-provider", empty.as_slice())];
        assert!(!check_hook_results(&per_provider));
    }

    #[test]
    fn test_check_hook_results_individual_error_flags_failure() {
        let results = vec![
            InstallResult {
                provider_name: "Cursor".to_string(),
                hooktype: "rules",
                result: Ok(()),
            },
            InstallResult {
                provider_name: "Cursor".to_string(),
                hooktype: "mdc",
                result: Err(anyhow!("permission denied")),
            },
        ];

        let per_provider = vec![("cursor", results.as_slice())];
        assert!(!check_hook_results(&per_provider));
    }

    #[test]
    fn test_check_hook_results_mixed_providers() {
        // One provider succeeds, another has empty results (failure)
        let good_results = vec![InstallResult {
            provider_name: "Claude Code".to_string(),
            hooktype: "pre-tool-use",
            result: Ok(()),
        }];

        let empty: Vec<InstallResult> = vec![];

        let per_provider = vec![
            ("claude-code", good_results.as_slice()),
            ("broken-provider", empty.as_slice()),
        ];
        assert!(!check_hook_results(&per_provider));
    }

    #[test]
    fn test_check_hook_results_no_providers_is_success() {
        // No providers at all — nothing failed
        let per_provider: Vec<(&str, &[InstallResult])> = vec![];
        assert!(check_hook_results(&per_provider));
    }

    #[test]
    fn test_check_hook_results_multiple_providers_all_success() {
        let claude_results = vec![
            InstallResult {
                provider_name: "Claude Code".to_string(),
                hooktype: "pre-tool-use",
                result: Ok(()),
            },
        ];

        let cursor_results = vec![
            InstallResult {
                provider_name: "Cursor".to_string(),
                hooktype: "rules",
                result: Ok(()),
            },
            InstallResult {
                provider_name: "Cursor".to_string(),
                hooktype: "mdc",
                result: Ok(()),
            },
        ];

        let per_provider = vec![
            ("claude-code", claude_results.as_slice()),
            ("cursor", cursor_results.as_slice()),
        ];
        assert!(check_hook_results(&per_provider));
    }
}
