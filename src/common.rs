use std::{env, fs, path::PathBuf, sync::OnceLock};

use anyhow::{Context, Result, anyhow};
#[cfg(unix)]
use log::debug;

pub const PROJECT_NAME: &str = env!("CARGO_PKG_NAME");
pub const PROJECT_VERSION: &str = env!("GIT_VERSION");
pub const PROJECT_VERSION_HASH: &str = env!("GIT_HASH");

/// Returns the User-Agent header for HTTP requests: "viberails/VERSION (OS; ARCH)"
pub fn user_agent() -> &'static str {
    static USER_AGENT: OnceLock<String> = OnceLock::new();
    USER_AGENT.get_or_init(|| {
        format!(
            "{}/{} ({}; {})",
            PROJECT_NAME,
            PROJECT_VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

#[cfg(windows)]
pub const EXECUTABLE_NAME: &str = concat!(env!("CARGO_PKG_NAME"), ".exe");
#[cfg(not(windows))]
pub const EXECUTABLE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(windows)]
pub const EXECUTABLE_EXT: &str = ".exe";
#[cfg(not(windows))]
pub const EXECUTABLE_EXT: &str = "";

pub fn print_header() {
    println!("{PROJECT_NAME} {PROJECT_VERSION}");
}

pub fn display_authorize_help() {
    print_header();

    let exe = env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| PROJECT_NAME.to_string());

    println!();
    println!("  Not logged in.");
    println!();
    println!("  Run `{exe} init-team` to create a new team, or");
    println!("  Run `{exe} join-team <URL>` to join an existing team.");
    println!();
}

/// Returns the project data directory, creating it with secure permissions if needed.
///
/// On Unix, creates the directory with mode 0700 (owner only) to protect
/// sensitive files like logs and cached data.
///
/// Parameters: None
///
/// Returns: Path to `~/.local/share/viberails` (or equivalent)
pub fn project_data_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_dir().ok_or_else(|| anyhow!("Unable to determine data directory. Ensure XDG_DATA_HOME or HOME environment variable is set"))?;

    let project_data_dir = data_dir.join(PROJECT_NAME);

    // Create directory with secure permissions (0700 on Unix)
    create_secure_directory(&project_data_dir)?;

    Ok(project_data_dir)
}

/// Returns the project config directory, creating it with secure permissions if needed.
///
/// On Unix, creates the directory with mode 0700 (owner only) to protect
/// sensitive config files like credentials and API keys.
///
/// Parameters: None
///
/// Returns: Path to `~/.config/viberails` (or equivalent)
pub fn project_config_dir() -> Result<PathBuf> {
    let data_dir = dirs::config_dir().ok_or_else(|| anyhow!("Unable to determine config directory. Ensure XDG_CONFIG_HOME or HOME environment variable is set"))?;

    let project_config_dir = data_dir.join(PROJECT_NAME);

    // Create directory with secure permissions (0700 on Unix)
    create_secure_directory(&project_config_dir)?;

    Ok(project_config_dir)
}

/// Creates a directory with secure permissions (0700 on Unix).
///
/// This function creates the directory if it doesn't exist, and ensures
/// permissions are set to owner-only (0700) even if the directory
/// already exists with different permissions.
///
/// Parameters:
///   - `dir`: Path to the directory to create/secure
///
/// Returns: `Ok(())` on success, Err on I/O failure
#[cfg(unix)]
fn create_secure_directory(dir: &std::path::Path) -> Result<()> {
    use std::fs::DirBuilder;
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::fs::PermissionsExt;

    let dir_exists = dir.exists();
    debug!(
        "Creating secure directory: {} (exists={})",
        dir.display(),
        dir_exists
    );

    // DirBuilder with mode sets permissions atomically at creation
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700);

    // create() is idempotent - succeeds if dir exists with any permissions
    builder
        .create(dir)
        .with_context(|| format!("Unable to create directory: {}", dir.display()))?;

    // Check current permissions before fixing
    if dir_exists
        && let Ok(metadata) = fs::metadata(dir)
    {
        let current_mode = metadata.permissions().mode() & 0o777;
        if current_mode != 0o700 {
            debug!(
                "Fixing directory permissions: {} (current={current_mode:o}, target=0700)",
                dir.display()
            );
        }
    }

    // Always verify/fix permissions (handles pre-existing directories)
    let perms = fs::Permissions::from_mode(0o700);
    fs::set_permissions(dir, perms)
        .with_context(|| format!("Unable to set permissions on directory: {}", dir.display()))?;

    debug!("Directory secured with 0700 permissions: {}", dir.display());
    Ok(())
}

/// Creates a directory (non-Unix version without special permissions).
///
/// Parameters:
///   - `dir`: Path to the directory to create
///
/// Returns: `Ok(())` on success, Err on I/O failure
#[cfg(not(unix))]
fn create_secure_directory(dir: &std::path::Path) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("Unable to create directory: {}", dir.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_create_secure_directory_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let test_dir = temp_dir.path().join("secure_test");

        // Create directory with secure permissions
        create_secure_directory(&test_dir).unwrap();

        let perms = std::fs::metadata(&test_dir).unwrap().permissions();
        let mode = perms.mode() & 0o777;

        assert_eq!(
            mode, 0o700,
            "Directory should have 0o700 permissions, got: {:o}",
            mode
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_create_secure_directory_fixes_insecure_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let test_dir = temp_dir.path().join("insecure_test");

        // Create directory with insecure permissions first
        std::fs::create_dir_all(&test_dir).unwrap();
        std::fs::set_permissions(&test_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Verify we were able to set insecure permissions (some CI environments block this)
        let mode_before = std::fs::metadata(&test_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;

        if mode_before != 0o755 {
            // Platform restrictions prevent setting insecure permissions
            eprintln!(
                "Skipping test: platform prevented setting insecure permissions (got {:o}, expected 0o755)",
                mode_before
            );
            return;
        }

        // Call create_secure_directory - should fix permissions
        create_secure_directory(&test_dir).unwrap();

        // Verify permissions are now secure
        let mode_after = std::fs::metadata(&test_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode_after, 0o700,
            "Directory permissions should be fixed to 0o700, got: {:o}",
            mode_after
        );
    }

    #[test]
    fn test_create_secure_directory_creates_nested_directories() {
        let temp_dir = tempfile::tempdir().unwrap();
        let nested_dir = temp_dir.path().join("a").join("b").join("c");

        // Create nested directory structure
        create_secure_directory(&nested_dir).unwrap();

        assert!(nested_dir.exists(), "Nested directory should exist");
        assert!(
            nested_dir.is_dir(),
            "Nested path should be a directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_create_secure_directory_is_idempotent() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let test_dir = temp_dir.path().join("idempotent_test");

        // Create twice - should succeed both times
        create_secure_directory(&test_dir).unwrap();
        create_secure_directory(&test_dir).unwrap();

        let perms = std::fs::metadata(&test_dir).unwrap().permissions();
        let mode = perms.mode() & 0o777;

        assert_eq!(
            mode, 0o700,
            "Directory should still have 0o700 permissions after second call, got: {:o}",
            mode
        );
    }
}
