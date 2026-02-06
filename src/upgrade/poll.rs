//! Implementation details for the auto-upgrade mechanism.
//!
//! Contains downloading, checksum verification, locking, atomic binary
//! replacement, and all the platform-specific helpers.

use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::sleep,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use log::{info, warn};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tiny_http::StatusCode;

use super::UpgradeConfig;
use crate::{
    common::{EXECUTABLE_EXT, EXECUTABLE_NAME, PROJECT_NAME, PROJECT_VERSION, user_agent},
    default::get_embedded_default,
    hooks::binary_location,
};

pub(super) const DEF_UPGRADE_CHECK: Duration = Duration::from_hours(1);

const DEF_COPY_ATTEMPTS: usize = 4;
const LOCK_FILE_NAME: &str = ".viberails.upgrade.lock";
const DOWNLOAD_TIMEOUT_SECS: u64 = 30;

/// Environment variable to allow upgrades without checksum verification.
/// Only for development/testing - production builds should never set this.
const ENV_ALLOW_MISSING_CHECKSUM: &str = "VB_ALLOW_MISSING_CHECKSUM";

#[derive(Deserialize)]
struct ReleaseInfo {
    version: String,
    #[serde(default)]
    checksums: HashMap<String, String>,
}

/// Returns the CPU architecture name for download URLs.
///
/// Parameters: None
///
/// Returns: Architecture string ("x64", "arm64", or raw arch name)
pub(super) fn get_arch() -> &'static str {
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
pub(crate) fn move_file_replace_windows(src: &Path, dst: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
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
        bail!("MoveFileExW failed: {}", std::io::Error::last_os_error());
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
pub(super) fn verify_checksum(file_path: &Path, expected_hash: &str) -> Result<()> {
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
pub(super) fn is_process_running(pid: u32) -> bool {
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
pub(super) fn is_process_running(pid: u32) -> bool {
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
pub(super) struct UpgradeLock {
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
    pub(super) fn acquire() -> Result<Option<Self>> {
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
pub(super) fn atomic_replace_binary(src: &Path, dst: &Path) -> Result<()> {
    let dst_dir = dst
        .parent()
        .context("Unable to get destination directory")?;

    // Step 1: Copy new binary to temp file in same directory
    // Generate random suffix to avoid collisions
    let random_suffix: u32 = rand::rng().random();
    let temp_name = format!(".{PROJECT_NAME}_new_{random_suffix:08x}{EXECUTABLE_EXT}");
    let temp_path = dst_dir.join(&temp_name);

    // Copy to temp file
    let copy_result = fs::copy(src, &temp_path).with_context(|| {
        format!(
            "Unable to copy {} to {}",
            src.display(),
            temp_path.display()
        )
    });

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
            return Err(e)
                .with_context(|| format!("Unable to replace binary at {}", dst.display()));
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

/// Downloads release info, compares versions, downloads new binary if needed,
/// verifies checksum, and performs atomic replacement with rollback support.
///
/// Parameters:
///   - `force`: If true, skip version check and always download/install
///   - `verbose`: If true, print progress to stdout
///
/// Returns: Upgrade result indicating what happened
pub(super) fn self_upgrade_with_force(force: bool, verbose: bool) -> Result<super::UpgradeResult> {
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
        return Ok(super::UpgradeResult::AlreadyLatest {
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
            warn!("Unable to replace binary at {} ({e})", dst.display());

            if 0 == attempts {
                return Err(e);
            }
        }

        attempts = attempts.saturating_sub(1);
        sleep(Duration::from_secs(5));
    }

    // Record that an upgrade just succeeded
    if let Err(e) = UpgradeConfig::load().record_upgrade() {
        warn!("unable to save upgrade state: {e}");
    }

    // Return appropriate result
    if is_reinstall {
        Ok(super::UpgradeResult::Reinstalled {
            version: release.version,
        })
    } else {
        Ok(super::UpgradeResult::Upgraded {
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
#[cfg(test)]
pub(super) fn is_file_older_than(path: &Path, max_age: &Duration) -> bool {
    use std::time::SystemTime;

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
pub(super) fn upgrade_binary_path() -> Result<PathBuf> {
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
pub(super) fn previous_upgrade_cleanup() {
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
pub(super) fn spawn_upgrade_with_force(force: bool) -> Result<()> {
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
