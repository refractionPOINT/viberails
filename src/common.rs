use std::{env, fs, path::PathBuf, sync::OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use log::info;
#[cfg(unix)]
use log::{debug, warn};

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

/// Environment variable to override the config directory.
///
/// Opt-in escape hatch for testing and CI environments where platform
/// config APIs (e.g. Windows `SHGetKnownFolderPath`) ignore env var
/// overrides. In production, leave this unset to use the secure default.
///
/// Example usage in tests:
///   export VIBERAILS_CONFIG_DIR="/tmp/test-config/viberails"
const ENV_CONFIG_DIR_OVERRIDE: &str = "VIBERAILS_CONFIG_DIR";

/// Returns the project config directory, creating it with secure permissions if needed.
///
/// If `VIBERAILS_CONFIG_DIR` is set, uses that path directly (validated for
/// safety). Otherwise falls back to `dirs::config_dir()/viberails`.
///
/// On Unix, creates the directory with mode 0700 (owner only) to protect
/// sensitive config files like credentials and API keys.
///
/// Parameters: None
///
/// Returns: Path to `~/.config/viberails` (or platform equivalent)
pub fn project_config_dir() -> Result<PathBuf> {
    let project_config_dir = if let Ok(override_dir) = env::var(ENV_CONFIG_DIR_OVERRIDE) {
        let path = PathBuf::from(&override_dir);

        if !path.is_absolute() {
            bail!("{ENV_CONFIG_DIR_OVERRIDE} must be an absolute path: {override_dir}");
        }
        // Reject path traversal
        for component in path.components() {
            if let std::path::Component::ParentDir = component {
                bail!("{ENV_CONFIG_DIR_OVERRIDE} contains parent directory references: {override_dir}");
            }
        }

        info!("Using config directory override from {ENV_CONFIG_DIR_OVERRIDE}: {}", path.display());
        path
    } else {
        let data_dir = dirs::config_dir().ok_or_else(|| anyhow!("Unable to determine config directory. Ensure XDG_CONFIG_HOME or HOME environment variable is set"))?;
        data_dir.join(PROJECT_NAME)
    };

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

/// Returns the validated home directory for the current user.
///
/// On Unix, validates that HOME environment variable (if set) matches the
/// actual home directory from the passwd database to prevent HOME injection attacks.
///
/// Parameters: None
///
/// Returns: Validated home directory path
#[cfg(unix)]
pub fn get_validated_home() -> Result<PathBuf> {
    use std::ffi::CStr;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStringExt;

    // Get the actual home directory from passwd database (not from $HOME)
    // Using getpwuid_r (reentrant/thread-safe) instead of getpwuid
    let passwd_home = unsafe {
        let uid = libc::getuid();

        // Determine buffer size for getpwuid_r
        // sysconf(_SC_GETPW_R_SIZE_MAX) returns suggested size, or -1 if indeterminate
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bufsize = match libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) {
            n if n > 0 => n as usize,
            _ => 16384, // Fallback to reasonable default
        };

        let mut buf = vec![0u8; bufsize];
        let mut pwd: MaybeUninit<libc::passwd> = MaybeUninit::uninit();
        let mut result: *mut libc::passwd = std::ptr::null_mut();

        let ret = libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr().cast::<libc::c_char>(),
            bufsize,
            std::ptr::addr_of_mut!(result),
        );

        if ret != 0 {
            return Err(anyhow!(
                "getpwuid_r failed for uid {uid}: {}",
                std::io::Error::from_raw_os_error(ret)
            ));
        }

        if result.is_null() {
            return Err(anyhow!("No passwd entry found for uid {uid}"));
        }

        let pwd = pwd.assume_init();
        if pwd.pw_dir.is_null() {
            return Err(anyhow!("passwd entry has null home directory"));
        }

        // Preserve non-UTF-8 paths by converting raw bytes to OsString
        let home_bytes = CStr::from_ptr(pwd.pw_dir).to_bytes().to_vec();
        std::ffi::OsString::from_vec(home_bytes)
    };

    let passwd_home = PathBuf::from(passwd_home);

    // Ensure home path is absolute (defense in depth)
    if !passwd_home.is_absolute() {
        bail!(
            "Home directory path must be absolute: {}",
            passwd_home.display()
        );
    }

    // Also get what dirs::home_dir() returns (which may use $HOME)
    let dirs_home = dirs::home_dir().ok_or_else(|| anyhow!("Unable to determine home directory"))?;

    // If HOME env var is set, verify it matches passwd entry
    if let Ok(env_home) = std::env::var("HOME") {
        let env_home = PathBuf::from(&env_home);

        // Canonicalize both paths for comparison (resolves symlinks)
        let canonical_passwd = passwd_home
            .canonicalize()
            .unwrap_or_else(|_| passwd_home.clone());
        let canonical_env = env_home.canonicalize().unwrap_or_else(|_| env_home.clone());

        if canonical_passwd != canonical_env {
            warn!(
                "HOME environment variable ({}) differs from passwd entry ({}), using passwd entry",
                env_home.display(),
                passwd_home.display()
            );
            return Ok(passwd_home);
        }
    }

    // Use dirs::home_dir() result if it matches or HOME wasn't set
    Ok(dirs_home)
}

/// Returns the validated home directory for the current user.
///
/// On Windows, uses the standard APIs via dirs crate.
///
/// Parameters: None
///
/// Returns: Validated home directory path
#[cfg(windows)]
pub fn get_validated_home() -> Result<PathBuf> {
    // On Windows, dirs::home_dir() uses proper Windows APIs (SHGetKnownFolderPath)
    // which are not vulnerable to environment variable injection
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Unable to determine home directory"))?;

    // Ensure home path is absolute (defense in depth)
    if !home.is_absolute() {
        bail!("Home directory path must be absolute: {}", home.display());
    }

    Ok(home)
}

/// Environment variable to override the binary installation directory.
///
/// This is an opt-in escape hatch for testing, CI, and environments that need
/// to control where binaries are installed. In production, leave this unset
/// to use the secure default (~/.local/bin with HOME validation).
///
/// Example usage in tests:
///   export VIBERAILS_BIN_DIR="/tmp/test-home/.local/bin"
const ENV_BIN_DIR_OVERRIDE: &str = "VIBERAILS_BIN_DIR";

/// Returns the validated binary installation directory.
///
/// Uses ~/.local/bin/ on Unix-like systems. Validates the home directory
/// to prevent HOME environment injection attacks.
///
/// If `VIBERAILS_BIN_DIR` environment variable is set, uses that path instead.
/// This is an opt-in override for testing and CI environments that need to
/// control the binary directory without modifying the system home.
///
/// Parameters: None
///
/// Returns: Path to the binary installation directory
pub fn validated_binary_dir() -> Result<PathBuf> {
    // Check for explicit override (for testing/CI)
    if let Ok(override_dir) = std::env::var(ENV_BIN_DIR_OVERRIDE) {
        let bin_dir = PathBuf::from(&override_dir);

        // Still validate the override path for safety
        if !bin_dir.is_absolute() {
            bail!(
                "{ENV_BIN_DIR_OVERRIDE} must be an absolute path: {override_dir}"
            );
        }

        // Check for path traversal attempts
        for component in bin_dir.components() {
            if let std::path::Component::ParentDir = component {
                bail!(
                    "{ENV_BIN_DIR_OVERRIDE} contains parent directory references: {override_dir}"
                );
            }
        }

        if !bin_dir.exists() {
            fs::create_dir_all(&bin_dir)
                .with_context(|| format!("Unable to create {}", bin_dir.display()))?;
        }

        info!(
            "Using binary directory override from {ENV_BIN_DIR_OVERRIDE}: {}",
            bin_dir.display()
        );
        return Ok(bin_dir);
    }

    // Default: use validated home directory
    let home = get_validated_home()?;

    // Ensure home path is absolute (defense in depth)
    if !home.is_absolute() {
        bail!("Home directory path must be absolute: {}", home.display());
    }

    // Check for path traversal attempts
    for component in home.components() {
        if let std::path::Component::ParentDir = component {
            bail!(
                "Home directory path contains parent directory references: {}",
                home.display()
            );
        }
    }

    let local_bin = home.join(".local").join("bin");

    if !local_bin.exists() {
        fs::create_dir_all(&local_bin)
            .with_context(|| format!("Unable to create {}", local_bin.display()))?;
    }

    Ok(local_bin)
}
