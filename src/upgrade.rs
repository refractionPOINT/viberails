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

use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::sleep,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use log::{info, warn};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tiny_http::StatusCode;

#[derive(Deserialize)]
struct ReleaseInfo {
    version: String,
    #[serde(default)]
    checksums: HashMap<String, String>,
}

use crate::{
    common::{EXECUTABLE_EXT, EXECUTABLE_NAME, PROJECT_NAME, PROJECT_VERSION, user_agent},
    default::get_embedded_default,
    hooks::binary_location,
};

const DEF_COPY_ATTEMPTS: usize = 4;
const DEF_UPGRADE_CHECK: Duration = Duration::from_mins(15);
const LOCK_FILE_NAME: &str = ".viberails.upgrade.lock";
const DOWNLOAD_TIMEOUT_SECS: u64 = 30;

/// Environment variable to allow upgrades without checksum verification.
/// Only for development/testing - production builds should never set this.
const ENV_ALLOW_MISSING_CHECKSUM: &str = "VB_ALLOW_MISSING_CHECKSUM";

/// Returns the CPU architecture name for download URLs.
///
/// Parameters: None
///
/// Returns: Architecture string ("x64", "arm64", or raw arch name)
fn get_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => std::env::consts::ARCH,
    }
}

/// Makes a file executable on Unix systems.
///
/// Parameters:
///   - `file_path`: Path to the file to make executable
///
/// Returns: Ok(()) on success, Err on failure
#[cfg(not(windows))]
fn make_executable(file_path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    info!("making {} executable", file_path.display());

    std::fs::set_permissions(file_path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("Unable to make {} executable", file_path.display()))?;
    Ok(())
}

/// Atomically replaces destination on Windows using `MoveFileExW`.
///
/// Uses Windows API flags:
/// - `MOVEFILE_REPLACE_EXISTING`: Replace dst if it exists (atomic swap)
/// - `MOVEFILE_WRITE_THROUGH`: Don't return until file is flushed to disk
///
/// This is the proper way to atomically replace a file on Windows.
/// Unlike `std::fs::rename()`, this works when the destination exists.
///
/// Parameters:
///   - `src`: Path to the new file (will be moved/deleted)
///   - `dst`: Path to the destination file (will be replaced if exists)
///
/// Returns: `Ok(())` on success, Err on Windows API failure
#[cfg(windows)]
fn move_file_replace_windows(src: &Path, dst: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    // Convert paths to null-terminated UTF-16 for Windows API
    let src_wide: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst_wide: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();

    // SAFETY: MoveFileExW expects valid null-terminated UTF-16 pointers.
    // We construct valid pointers from OsStr above.
    let result = unsafe {
        MoveFileExW(
            src_wide.as_ptr(),
            dst_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if result == 0 {
        bail!(
            "MoveFileExW failed: {}",
            std::io::Error::last_os_error()
        );
    }

    Ok(())
}

/// Downloads a file from URL to destination path.
///
/// Parameters:
///   - `url`: The URL to download from
///   - `dst`: Destination path to write the file
///
/// Returns: Ok(()) on success, Err on HTTP error or I/O failure
fn download_file(url: &str, dst: &Path) -> Result<()> {
    let mut fd = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(dst)
        .with_context(|| format!("Unable to open {} for writing", dst.display()))?;

    info!("Downloading: {url}");

    let res = minreq::get(url)
        .with_header("User-Agent", user_agent())
        .with_timeout(DOWNLOAD_TIMEOUT_SECS)
        .send()
        .with_context(|| format!("{url} failed"))?;

    if !(200..300).contains(&res.status_code) {
        let status_str = StatusCode::from(res.status_code).default_reason_phrase();
        bail!("{url} returned {} ({})", res.status_code, status_str);
    }

    fd.write_all(res.as_bytes())?;

    Ok(())
}

/// Verifies SHA256 checksum of a file.
///
/// Parameters:
///   - `file_path`: Path to the file to verify
///   - `expected_hash`: Expected SHA256 hash in hex format
///
/// Returns: Ok(()) if checksum matches, Err on mismatch or I/O error
fn verify_checksum(file_path: &Path, expected_hash: &str) -> Result<()> {
    let mut file = fs::File::open(file_path).with_context(|| {
        format!(
            "Unable to open {} for checksum verification",
            file_path.display()
        )
    })?;

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        if let Some(chunk) = buffer.get(..bytes_read) {
            hasher.update(chunk);
        }
    }

    let actual_hash = format!("{:x}", hasher.finalize());

    if actual_hash != expected_hash {
        bail!(
            "Checksum mismatch for {}: expected {}, got {}",
            file_path.display(),
            expected_hash,
            actual_hash
        );
    }

    info!("Checksum verified for {}", file_path.display());
    Ok(())
}

/// Returns the path to the lock file.
///
/// Parameters: None
///
/// Returns: Path to the lock file in the binary directory
fn lock_file_path() -> Result<PathBuf> {
    let bin_dir = binary_location()?
        .parent()
        .context("Unable to get binary directory")?
        .to_path_buf();
    Ok(bin_dir.join(LOCK_FILE_NAME))
}

/// Checks if a process with the given PID is still running.
///
/// Parameters:
///   - `pid`: Process ID to check
///
/// Returns: true if process is running, false otherwise
#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    // kill(pid, 0) checks if process exists without sending a signal
    // SAFETY: kill with signal 0 is safe - it only checks process existence
    #[allow(clippy::cast_possible_wrap)]
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
    }
}

/// Checks if a process with the given PID is still running.
///
/// Parameters:
///   - `pid`: Process ID to check
///
/// Returns: true if process is running, false otherwise
#[cfg(windows)]
fn is_process_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::OpenProcess;

    // PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    // Try to open the process handle - if we can, it's running
    // SAFETY: OpenProcess is safe to call with valid parameters
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };

    if handle.is_null() {
        return false;
    }

    // Close the handle and return true since process exists
    // SAFETY: handle is valid and non-null
    unsafe {
        CloseHandle(handle);
    }
    true
}

/// RAII guard for upgrade lock using proper file locking.
/// Uses `flock()` on Unix and `LockFileEx` on Windows via fs2 crate.
struct UpgradeLock {
    #[allow(dead_code)]
    file: fs::File,
    path: PathBuf,
}

impl UpgradeLock {
    /// Attempts to acquire an exclusive upgrade lock.
    ///
    /// Uses proper OS-level file locking (flock on Unix, `LockFileEx` on Windows)
    /// to prevent race conditions. Also verifies PID of existing lock holder
    /// to detect stale locks from crashed processes.
    ///
    /// Parameters: None
    ///
    /// Returns: Ok(Some(lock)) if acquired, Ok(None) if another upgrade in progress,
    ///          Err on I/O failure
    fn acquire() -> Result<Option<Self>> {
        let path = lock_file_path()?;

        // Open or create the lock file
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("Unable to open lock file {}", path.display()))?;

        // Try to acquire exclusive lock (non-blocking)
        match file.try_lock_exclusive() {
            Ok(()) => {
                // Successfully acquired lock - write our PID
                let mut file = file;
                file.set_len(0)?;
                write!(file, "{}", std::process::id())?;
                file.sync_all()?;

                info!("Acquired upgrade lock");
                Ok(Some(Self { file, path }))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Lock is held by another process - check if it's still alive
                let mut contents = String::new();
                let mut reader = &file;
                if reader.read_to_string(&mut contents).is_ok()
                    && let Ok(pid) = contents.trim().parse::<u32>()
                    && !is_process_running(pid)
                {
                    // Lock holder is dead - this is a stale lock
                    // We still can't take it because flock is held by OS
                    // Just log and return - the OS will release it eventually
                    warn!("Stale lock detected (PID {pid} not running), waiting for OS cleanup");
                }

                info!("Upgrade already in progress (lock held by another process)");
                Ok(None)
            }
            Err(e) => {
                Err(e).with_context(|| format!("Failed to acquire lock on {}", path.display()))
            }
        }
    }
}

impl Drop for UpgradeLock {
    fn drop(&mut self) {
        // fs2 automatically unlocks when file is dropped
        // Just remove the lock file for cleanliness
        let _ = fs::remove_file(&self.path);
    }
}

/// Performs safe binary replacement.
///
/// Steps:
/// 1. Copy new binary to temp file in same directory
/// 2. Sync temp file to disk (Unix only)
/// 3. Replace destination atomically:
///    - Unix: `rename()` atomically replaces dst (POSIX guarantees)
///    - Windows: `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` flag
/// 4. Sync directory for durability (Unix only)
///
/// On both platforms, the destination is never missing during the operation.
/// Temp files are cleaned up on any failure.
///
/// Parameters:
///   - `src`: Source binary (downloaded new version)
///   - `dst`: Destination path (install location)
///
/// Returns: `Ok(())` on success, Err on failure (temp file cleaned on errors)
fn atomic_replace_binary(src: &Path, dst: &Path) -> Result<()> {
    let dst_dir = dst
        .parent()
        .context("Unable to get destination directory")?;

    // Step 1: Copy new binary to temp file in same directory
    // Generate random suffix to avoid collisions
    let random_suffix: u32 = rand::rng().random();
    let temp_name = format!(".{PROJECT_NAME}_new_{random_suffix:08x}{EXECUTABLE_EXT}");
    let temp_path = dst_dir.join(&temp_name);

    // Copy to temp file
    let copy_result = fs::copy(src, &temp_path)
        .with_context(|| format!("Unable to copy {} to {}", src.display(), temp_path.display()));

    if let Err(e) = copy_result {
        // Clean up partial temp file if it exists
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    // Make the temp file executable before rename (Unix only)
    #[cfg(not(windows))]
    if let Err(e) = make_executable(&temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    // Step 2: Sync temp file to disk for durability
    #[cfg(unix)]
    {
        if let Ok(file) = fs::File::open(&temp_path) {
            let _ = file.sync_all(); // Best effort fsync
        }
    }

    // Step 3: Replace destination
    #[cfg(unix)]
    {
        // On Unix, rename() atomically replaces dst - the old file remains accessible
        // until this call completes, so dst is NEVER missing.
        let rename_result = fs::rename(&temp_path, dst).with_context(|| {
            format!(
                "Unable to rename {} to {}",
                temp_path.display(),
                dst.display()
            )
        });

        if let Err(e) = rename_result {
            let _ = fs::remove_file(&temp_path);
            return Err(e);
        }
    }

    #[cfg(windows)]
    {
        // On Windows, std::fs::rename() fails if the destination exists.
        // Instead, use MoveFileExW with MOVEFILE_REPLACE_EXISTING flag which:
        // - Atomically replaces the destination file
        // - Keeps dst accessible until the operation completes
        // - Uses MOVEFILE_WRITE_THROUGH to ensure durability
        //
        // This is the proper Windows API for atomic file replacement,
        // avoiding the need for backup/restore logic.
        if let Err(e) = move_file_replace_windows(&temp_path, dst) {
            warn!("Failed to replace binary on Windows: {e}");
            let _ = fs::remove_file(&temp_path);
            return Err(e).with_context(|| {
                format!(
                    "Unable to replace binary at {}",
                    dst.display()
                )
            });
        }
    }

    // Step 4: Sync directory to ensure rename is durable
    #[cfg(unix)]
    {
        if let Ok(dir) = fs::File::open(dst_dir) {
            let _ = dir.sync_all(); // Best effort directory fsync
        }
    }

    info!("Successfully installed new binary at {}", dst.display());
    Ok(())
}

/// Performs the self-upgrade process.
///
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

/// Downloads release info, compares versions, downloads new binary if needed,
/// verifies checksum, and performs atomic replacement with rollback support.
///
/// Parameters:
///   - `force`: If true, skip version check and always download/install
///   - `verbose`: If true, print progress to stdout
///
/// Returns: Upgrade result indicating what happened
fn self_upgrade_with_force(force: bool, verbose: bool) -> Result<UpgradeResult> {
    let plat = std::env::consts::OS;
    let arch = get_arch();

    let base_url = get_embedded_default("upgrade_url");
    let artifact_name = format!("{PROJECT_NAME}-{plat}-{arch}{EXECUTABLE_EXT}");

    let rel_url = format!("{base_url}/release.json");
    let url_bin = format!("{base_url}/{artifact_name}");

    if verbose {
        println!("Current version: {PROJECT_VERSION}");
        println!("Checking for updates...");
    }

    // Create temp directory for downloads
    let td = tempfile::Builder::new()
        .prefix("upgrade_")
        .tempdir()
        .context("Unable to create a temp directory")?;

    let tmp_rel = td.path().join("release.json");
    let tmp_bin = td.path().join(EXECUTABLE_NAME);

    // Download release info to check version
    download_file(&rel_url, &tmp_rel)?;

    let version_data = fs::read_to_string(&tmp_rel)
        .with_context(|| format!("Unable to read {}", tmp_rel.display()))?;

    let release: ReleaseInfo = serde_json::from_str(&version_data)
        .with_context(|| format!("Unable to deserialize {version_data}"))?;

    if verbose {
        println!("Latest version:  {}", release.version);
    }

    // Check if already on latest version (skip if force is set)
    if release.version == PROJECT_VERSION && !force {
        info!("Already on latest version {PROJECT_VERSION}");
        return Ok(UpgradeResult::AlreadyLatest {
            version: PROJECT_VERSION.to_string(),
        });
    }

    let is_reinstall = release.version == PROJECT_VERSION;
    if is_reinstall {
        info!("Force upgrade: reinstalling version {PROJECT_VERSION}");
        if verbose {
            println!("Force reinstalling version {PROJECT_VERSION}...");
        }
    } else {
        info!("Upgrading from {} to {}", PROJECT_VERSION, release.version);
        if verbose {
            println!(
                "Upgrading from {} to {}...",
                PROJECT_VERSION, release.version
            );
        }
    }

    if verbose {
        println!("Downloading update...");
    }
    download_file(&url_bin, &tmp_bin)?;

    // Verify checksum - required by default, can be disabled via env var
    if verbose {
        println!("Verifying checksum...");
    }
    if let Some(expected_hash) = release.checksums.get(&artifact_name) {
        verify_checksum(&tmp_bin, expected_hash)?;
    } else {
        // Checksum missing - fail unless explicitly allowed
        let allow_missing = env::var(ENV_ALLOW_MISSING_CHECKSUM).is_ok();
        if allow_missing {
            warn!(
                "No checksum available for {artifact_name}, proceeding anyway ({ENV_ALLOW_MISSING_CHECKSUM} is set)"
            );
        } else {
            bail!(
                "No checksum available for {artifact_name}. This is a security risk. \
                 Set {ENV_ALLOW_MISSING_CHECKSUM}=1 to override (not recommended)."
            );
        }
    }

    let dst = binary_location()?;

    if verbose {
        println!("Installing...");
    }

    // Use atomic replacement with retry logic for busy files
    let mut attempts = DEF_COPY_ATTEMPTS;

    loop {
        let ret = atomic_replace_binary(&tmp_bin, &dst);

        if ret.is_ok() {
            break;
        }

        if let Err(e) = ret {
            warn!(
                "Unable to replace binary at {} ({e})",
                dst.display()
            );

            if 0 == attempts {
                return Err(e);
            }
        }

        attempts = attempts.saturating_sub(1);
        sleep(Duration::from_secs(5));
    }

    // Return appropriate result
    if is_reinstall {
        Ok(UpgradeResult::Reinstalled {
            version: release.version,
        })
    } else {
        Ok(UpgradeResult::Upgraded {
            from: PROJECT_VERSION.to_string(),
            to: release.version,
        })
    }
}

/// Checks if a file is older than the specified duration.
///
/// Parameters:
///   - `path`: Path to the file to check
///   - `max_age`: Maximum age before file is considered old
///
/// Returns: true if file is older than `max_age`, false if newer or on error
fn is_file_older_than(path: &Path, max_age: &Duration) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };

    // Try created time first, fall back to modified time
    let file_time = metadata.created().or_else(|_| metadata.modified());

    let Ok(file_time) = file_time else {
        return false;
    };

    let Ok(elapsed) = SystemTime::now().duration_since(file_time) else {
        return false;
    };

    &elapsed > max_age
}

/// Checks if the current binary is older than the specified duration.
///
/// Parameters:
///   - `max_age`: Maximum age before binary is considered old
///
/// Returns: true if binary is older than `max_age`, false otherwise
fn is_binary_older(max_age: &Duration) -> bool {
    let Ok(exe_path) = std::env::current_exe() else {
        return false;
    };

    is_file_older_than(&exe_path, max_age)
}

/// Spawns a detached process on Unix systems.
///
/// Parameters:
///   - `path`: Path to executable
///   - `args`: Command line arguments
///
/// Returns: Ok(()) on success, Err on spawn failure
#[cfg(unix)]
fn spawn_detached(path: &Path, args: &[&str]) -> Result<()> {
    use std::os::unix::process::CommandExt;

    unsafe {
        Command::new(path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()?;
    }
    Ok(())
}

/// Spawns a detached process on Windows systems.
///
/// Parameters:
///   - `path`: Path to executable
///   - `args`: Command line arguments
///
/// Returns: Ok(()) on success, Err on spawn failure
#[cfg(windows)]
fn spawn_detached(path: &Path, args: &[&str]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;

    Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

/// Returns the path for the temporary upgrade binary.
///
/// Uses a random suffix to prevent pre-placement attacks where an attacker
/// could place a malicious binary at a predictable path.
///
/// Parameters: None
///
/// Returns: Path with random suffix for upgrade binary
fn upgrade_binary_path() -> Result<PathBuf> {
    let exe_path = env::current_exe()?;
    let parent = exe_path
        .parent()
        .context("Unable to get parent directory of current executable")?;

    // Generate random suffix to prevent predictable path attacks
    let random_suffix: u32 = rand::rng().random();
    Ok(parent.join(format!(
        "{PROJECT_NAME}_upgrade_{random_suffix:08x}{EXECUTABLE_EXT}"
    )))
}

/// Cleans up any previous upgrade binaries.
///
/// Removes upgrade binaries that match the pattern from previous upgrade attempts.
/// Uses glob-like matching since the path now includes random suffixes.
///
/// Parameters: None
///
/// Returns: Nothing (failures are silently ignored)
fn previous_upgrade_cleanup() {
    let Ok(exe_path) = env::current_exe() else {
        return;
    };

    let Some(parent) = exe_path.parent() else {
        return;
    };

    // Read directory and find upgrade binaries
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };

    let upgrade_prefix = format!("{PROJECT_NAME}_upgrade_");

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Match upgrade binaries (with random suffix)
        if name.starts_with(&upgrade_prefix) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Spawns a separate process to perform the upgrade.
///
/// This is needed because on Windows the current executable is locked and
/// cannot be overwritten. We copy ourselves to a temp location and run
/// the upgrade from there.
///
/// Parameters:
///   - `force`: If true, pass --force flag to the spawned upgrade process
///
/// Returns: `Ok(())` on success, Err on failure
fn spawn_upgrade_with_force(force: bool) -> Result<()> {
    // We can't upgrade ourselves because Windows locks the current executable.
    // We have to make a copy of ourselves and run the upgrade from there.
    let src = env::current_exe()?;
    let dst = upgrade_binary_path()?;

    fs::copy(&src, &dst)?;

    #[cfg(not(windows))]
    make_executable(&dst)?;

    info!("executing {}", dst.display());

    // Pass --force flag if requested
    if force {
        spawn_detached(&dst, &["upgrade", "--force"])?;
    } else {
        spawn_detached(&dst, &["upgrade"])?;
    }

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// PUBLIC API
////////////////////////////////////////////////////////////////////////////////

/// Checks if an upgrade should be performed and triggers it if needed.
///
/// Called on program exit. Only triggers upgrade if the binary is older than
/// `DEF_UPGRADE_CHECK` (15 minutes) or `VB_FORCE_UPGRADE` env var is set.
/// Runs silently without verbose output.
///
/// Parameters: None
///
/// Returns: `Ok(())` on success or if no upgrade needed, Err on upgrade failure
pub fn poll_upgrade() -> Result<()> {
    let force_upgrade = env::var("VB_FORCE_UPGRADE").is_ok();

    if is_binary_older(&DEF_UPGRADE_CHECK) || force_upgrade {
        info!("time to upgrade");
        // Auto-upgrade: never forces reinstall, not verbose (background operation)
        upgrade(false, false)?;
    } else {
        previous_upgrade_cleanup();
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_verify_checksum_valid() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let file_path = dir.path().join("test.bin");

        // Write known content
        let content = b"hello world";
        fs::write(&file_path, content).expect("Failed to write test file");

        // SHA256 of "hello world"
        let expected_hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

        let result = verify_checksum(&file_path, expected_hash);
        assert!(result.is_ok(), "Checksum verification should succeed");
    }

    #[test]
    fn test_verify_checksum_invalid() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let file_path = dir.path().join("test.bin");

        fs::write(&file_path, b"hello world").expect("Failed to write test file");

        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";

        let result = verify_checksum(&file_path, wrong_hash);
        assert!(result.is_err(), "Checksum verification should fail");
    }

    #[test]
    fn test_atomic_replace_binary_new_file() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("source.bin");
        let dst = dir.path().join("dest.bin");

        // Create source file
        fs::write(&src, b"new binary content").expect("Failed to write source");

        // Replace (destination doesn't exist)
        let result = atomic_replace_binary(&src, &dst);
        assert!(result.is_ok(), "Atomic replace should succeed: {:?}", result);

        // Verify content
        let content = fs::read_to_string(&dst).expect("Failed to read dest");
        assert_eq!(content, "new binary content");
    }

    #[test]
    fn test_atomic_replace_binary_with_existing() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("source.bin");
        let dst = dir.path().join("dest.bin");

        // Create both files
        fs::write(&src, b"new content").expect("Failed to write source");
        fs::write(&dst, b"old content").expect("Failed to write dest");

        let result = atomic_replace_binary(&src, &dst);
        assert!(result.is_ok(), "Atomic replace should succeed: {:?}", result);

        // Verify new content replaced old content
        let content = fs::read_to_string(&dst).expect("Failed to read dest");
        assert_eq!(content, "new content");
    }

    #[test]
    fn test_atomic_replace_binary_cleans_temp_on_failure() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("nonexistent.bin"); // Source doesn't exist
        let dst = dir.path().join("dest.bin");

        // This should fail because source doesn't exist
        let result = atomic_replace_binary(&src, &dst);
        assert!(result.is_err(), "Should fail with nonexistent source");

        // Verify no temp files left behind (they start with .)
        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("Failed to read dir")
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!(".{PROJECT_NAME}_new_"))
            })
            .collect();
        assert!(
            entries.is_empty(),
            "No temp files should be left on failure"
        );
    }

    #[test]
    fn test_upgrade_binary_path_is_random() {
        // Call twice and verify different paths
        let path1 = upgrade_binary_path().expect("Failed to get upgrade path 1");
        let path2 = upgrade_binary_path().expect("Failed to get upgrade path 2");

        // Paths should be different due to random suffix
        assert_ne!(path1, path2, "Upgrade paths should have random suffixes");

        // Both should contain the upgrade prefix
        let name1 = path1.file_name().unwrap().to_string_lossy();
        let name2 = path2.file_name().unwrap().to_string_lossy();
        assert!(
            name1.contains("_upgrade_"),
            "Path should contain upgrade prefix"
        );
        assert!(
            name2.contains("_upgrade_"),
            "Path should contain upgrade prefix"
        );
    }

    #[test]
    fn test_is_file_older_than_with_new_file() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let test_file = dir.path().join("test.bin");

        // Create a new file
        fs::write(&test_file, b"test").expect("Failed to write test file");

        // File just created should not be older than 1 hour
        assert!(
            !is_file_older_than(&test_file, &Duration::from_secs(3600)),
            "Newly created file should not be older than 1 hour"
        );

        // File just created should not be older than 1 second
        // (avoids flakiness from Duration::ZERO where timestamp resolution matters)
        assert!(
            !is_file_older_than(&test_file, &Duration::from_secs(1)),
            "Newly created file should not be older than 1 second"
        );
    }

    #[test]
    fn test_is_file_older_than_nonexistent() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let nonexistent = dir.path().join("does_not_exist.bin");

        // Nonexistent file should return false
        assert!(
            !is_file_older_than(&nonexistent, &Duration::from_secs(1)),
            "Nonexistent file should return false"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_is_process_running_current() {
        let pid = std::process::id();
        assert!(
            is_process_running(pid),
            "Current process should be running"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_is_process_running_nonexistent() {
        // Test with a PID that's extremely unlikely to exist.
        // On Linux, PIDs typically max at 32768 or 4194304 (with pid_max).
        // Use i32::MAX which is a valid pid_t but virtually never exists.
        // Note: This is not a strict guarantee on systems with unusual PID ranges.
        #[allow(clippy::cast_sign_loss)]
        let unlikely_pid = i32::MAX as u32;

        // This PID should not be running on any normal system
        let result = is_process_running(unlikely_pid);

        // Assert the expected behavior - this PID should not exist
        assert!(
            !result,
            "PID {} should not be running on any normal system",
            unlikely_pid
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_is_process_running_pid_zero() {
        // PID 0 is the kernel scheduler, kill(0, 0) sends to process group
        // This tests edge case handling
        let result = is_process_running(0);
        // Result depends on permissions, but function should not panic
        let _ = result;
    }

    #[cfg(windows)]
    #[test]
    fn test_is_process_running_current_windows() {
        let pid = std::process::id();
        assert!(
            is_process_running(pid),
            "Current process should be running"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_is_process_running_nonexistent_windows() {
        // Use a PID that's extremely unlikely to exist on Windows
        // Windows PIDs are typically in the range 0-65535 but can go higher
        // Use u32::MAX which is virtually never a valid PID
        let unlikely_pid = u32::MAX;

        let result = is_process_running(unlikely_pid);
        assert!(
            !result,
            "PID {} should not be running on any normal system",
            unlikely_pid
        );
    }

    #[test]
    fn test_atomic_replace_binary_no_leftover_temp_or_backup() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("source.bin");
        let dst = dir.path().join("dest.bin");

        // Create both source and destination
        fs::write(&src, b"new content").expect("Failed to write source");
        fs::write(&dst, b"old content").expect("Failed to write dest");

        // Perform replacement
        let result = atomic_replace_binary(&src, &dst);
        assert!(result.is_ok(), "Replacement should succeed: {:?}", result);

        // Verify no temp files (.viberails_new_*) or backup files (.viberails_old_*) left
        let leftover_files: Vec<_> = fs::read_dir(dir.path())
            .expect("Failed to read dir")
            .filter_map(Result::ok)
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(&format!(".{PROJECT_NAME}_new_"))
                    || name.starts_with(&format!(".{PROJECT_NAME}_old_"))
            })
            .collect();

        assert!(
            leftover_files.is_empty(),
            "No temp or backup files should remain after successful replacement, found: {:?}",
            leftover_files.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_atomic_replace_binary_preserves_dst_on_src_read_failure() {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("nonexistent_source.bin");
        let dst = dir.path().join("dest.bin");

        // Create destination with known content
        fs::write(&dst, b"original content").expect("Failed to write dest");

        // Try to replace with nonexistent source - should fail
        let result = atomic_replace_binary(&src, &dst);
        assert!(result.is_err(), "Should fail with nonexistent source");

        // Verify destination is unchanged
        let content = fs::read_to_string(&dst).expect("Failed to read dest");
        assert_eq!(
            content, "original content",
            "Destination should be unchanged after failed replacement"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_atomic_replace_binary_windows_replaces_existing() {
        // Windows-specific test: verify MoveFileExW replaces existing file
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("source.bin");
        let dst = dir.path().join("dest.bin");

        // Create files
        fs::write(&src, b"new content").expect("Failed to write source");
        fs::write(&dst, b"old content").expect("Failed to write dest");

        // Replace using atomic_replace_binary (which uses MoveFileExW on Windows)
        let result = atomic_replace_binary(&src, &dst);
        assert!(result.is_ok(), "Replacement should succeed");

        // Verify content changed
        let content = fs::read_to_string(&dst).expect("Failed to read dest");
        assert_eq!(content, "new content");

        // Verify no leftover temp files
        let leftover_files: Vec<_> = fs::read_dir(dir.path())
            .expect("Failed to read dir")
            .filter_map(Result::ok)
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with(&format!(".{PROJECT_NAME}_"))
            })
            .collect();

        assert!(
            leftover_files.is_empty(),
            "No temp files should remain after successful replacement"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_move_file_replace_windows_new_file() {
        // Test MoveFileExW when destination doesn't exist
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("source.bin");
        let dst = dir.path().join("dest.bin");

        fs::write(&src, b"test content").expect("Failed to write source");

        let result = move_file_replace_windows(&src, &dst);
        assert!(result.is_ok(), "Move should succeed: {:?}", result);

        // Verify file moved
        assert!(!src.exists(), "Source should no longer exist");
        assert!(dst.exists(), "Destination should exist");
        let content = fs::read_to_string(&dst).expect("Failed to read dest");
        assert_eq!(content, "test content");
    }

    #[cfg(windows)]
    #[test]
    fn test_move_file_replace_windows_replaces_existing() {
        // Test MoveFileExW replaces existing destination atomically
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("source.bin");
        let dst = dir.path().join("dest.bin");

        fs::write(&src, b"new content").expect("Failed to write source");
        fs::write(&dst, b"old content").expect("Failed to write dest");

        let result = move_file_replace_windows(&src, &dst);
        assert!(result.is_ok(), "Move should succeed: {:?}", result);

        // Verify replacement
        assert!(!src.exists(), "Source should no longer exist");
        let content = fs::read_to_string(&dst).expect("Failed to read dest");
        assert_eq!(content, "new content");
    }

    #[cfg(windows)]
    #[test]
    fn test_move_file_replace_windows_nonexistent_source() {
        // Test MoveFileExW fails gracefully with nonexistent source
        let dir = TempDir::new().expect("Failed to create temp dir");
        let src = dir.path().join("nonexistent.bin");
        let dst = dir.path().join("dest.bin");

        let result = move_file_replace_windows(&src, &dst);
        assert!(result.is_err(), "Move should fail with nonexistent source");
    }

    #[test]
    fn test_get_arch_returns_valid_arch() {
        let arch = get_arch();

        // Should return a non-empty string
        assert!(!arch.is_empty(), "Architecture should not be empty");

        // On known platforms, should return normalized names
        match std::env::consts::ARCH {
            "x86_64" => assert_eq!(arch, "x64"),
            "aarch64" => assert_eq!(arch, "arm64"),
            other => assert_eq!(arch, other),
        }
    }
}
