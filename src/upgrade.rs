use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread::sleep,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use log::{info, warn};
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

fn get_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => std::env::consts::ARCH,
    }
}

#[cfg(not(windows))]
fn make_executable(file_path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    info!("making {} executable", file_path.display());

    std::fs::set_permissions(file_path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("Unable to make {} executable", file_path.display()))?;
    Ok(())
}

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

fn lock_file_path() -> Result<std::path::PathBuf> {
    let bin_dir = binary_location()?
        .parent()
        .context("Unable to get binary directory")?
        .to_path_buf();
    Ok(bin_dir.join(LOCK_FILE_NAME))
}

struct UpgradeLock {
    path: std::path::PathBuf,
}

impl UpgradeLock {
    fn acquire() -> Result<Option<Self>> {
        let path = lock_file_path()?;

        // Check if lock file exists and is recent (less than 10 minutes old)
        if path.exists() {
            let is_recent = fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|elapsed| elapsed < Duration::from_mins(10));

            if is_recent {
                info!("Upgrade already in progress (lock file exists)");
                return Ok(None);
            }

            // Stale lock file, remove it
            let _ = fs::remove_file(&path);
        }

        // Create lock file
        fs::write(&path, format!("{}", std::process::id()))
            .with_context(|| format!("Unable to create lock file {}", path.display()))?;

        Ok(Some(Self { path }))
    }
}

impl Drop for UpgradeLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Returns None if already on latest version, Some(ReleaseInfo) if upgraded
fn self_upgrade() -> Result<Option<ReleaseInfo>> {
    let plat = std::env::consts::OS;
    let arch = get_arch();

    let base_url = get_embedded_default("upgrade_url");
    let artifact_name = format!("{PROJECT_NAME}-{plat}-{arch}{EXECUTABLE_EXT}");

    let rel_url = format!("{base_url}/release.json");
    let url_bin = format!("{base_url}/{artifact_name}");

    //
    // We'll save it to a tmp file first and then install it where it should
    // be if this works
    //
    let td = tempfile::Builder::new()
        .prefix("upgrade_")
        .tempdir()
        .context("Unable to create a temp directory")?;

    let tmp_rel = td.path().join("release.json");
    let tmp_bin = td.path().join(EXECUTABLE_NAME);

    //
    // Download the release info first to check version
    //
    download_file(&rel_url, &tmp_rel)?;

    let version_data = fs::read_to_string(&tmp_rel)
        .with_context(|| format!("Unable to read {}", tmp_rel.display()))?;

    let release: ReleaseInfo = serde_json::from_str(&version_data)
        .with_context(|| format!("Unable to deserialize {version_data}"))?;

    //
    // Check if we're already on the latest version
    //
    if release.version == PROJECT_VERSION {
        info!("Already on latest version {PROJECT_VERSION}");
        return Ok(None);
    }

    info!("Upgrading from {} to {}", PROJECT_VERSION, release.version);

    download_file(&url_bin, &tmp_bin)?;

    //
    // Verify checksum if available
    //
    if let Some(expected_hash) = release.checksums.get(&artifact_name) {
        verify_checksum(&tmp_bin, expected_hash)?;
    } else {
        warn!("No checksum available for {artifact_name}, skipping verification");
    }

    #[cfg(not(windows))]
    make_executable(&tmp_bin)?;

    let dst = binary_location()?;

    let mut attempts = DEF_COPY_ATTEMPTS;

    loop {
        let ret = fs::copy(&tmp_bin, &dst);

        if ret.is_ok() {
            break;
        }

        if let Err(e) = ret {
            warn!(
                "Unable to copy {} to {} ({e})",
                tmp_bin.display(),
                dst.display()
            );

            if 0 == attempts {
                return Err(e).with_context(|| {
                    format!("Unable to copy {} to {})", tmp_bin.display(), dst.display())
                })?;
            }
        }

        attempts = attempts.saturating_sub(1);
        sleep(Duration::from_secs(5));
    }

    Ok(Some(release))
}

fn is_binary_older(max_age: &Duration) -> bool {
    let Ok(exe_path) = std::env::current_exe() else {
        return false;
    };

    let Ok(metadata) = fs::metadata(&exe_path) else {
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

fn upgrade_binary_path() -> Result<std::path::PathBuf> {
    let exe_path = env::current_exe()?;
    let parent = exe_path
        .parent()
        .context("Unable to get parent directory of current executable")?;
    Ok(parent.join(format!("{PROJECT_NAME}_upgrade{EXECUTABLE_EXT}")))
}

fn previous_upgrade_cleanup() {
    let Ok(upgrade_bin) = upgrade_binary_path() else {
        return;
    };

    if upgrade_bin.exists() {
        let _ = fs::remove_file(&upgrade_bin);
    }
}

fn spawn_upgrade() -> Result<()> {
    //
    // We can't just upgrade our own because windows locks the current
    // executable to we can't simply overwrite it. We have to make a copy of
    // ourselves next to the current executable and run that instead.
    //
    let src = env::current_exe()?;
    let dst = upgrade_binary_path()?;

    fs::copy(&src, &dst)?;

    #[cfg(not(windows))]
    make_executable(&dst)?;

    info!("executing {}", dst.display());
    spawn_detached(&dst, &["upgrade"])?;

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////<
// PUB
////////////////////////////////////////////////////////////////////////////////

pub fn poll_upgrade() -> Result<()> {
    let force_upgrade = env::var("VB_FORCE_UPGRADE").is_ok();

    if is_binary_older(&DEF_UPGRADE_CHECK) || force_upgrade {
        //
        // time to try to upgrade
        //
        info!("time to upgrade");
        upgrade()?;
    } else {
        previous_upgrade_cleanup();
    }

    Ok(())
}

pub fn upgrade() -> Result<()> {
    info!("Upgrading");

    previous_upgrade_cleanup();

    // Acquire upgrade lock to prevent concurrent upgrades
    let Some(_lock) = UpgradeLock::acquire()? else {
        return Ok(());
    };

    let bin_location = binary_location()?;
    let bin_current = env::current_exe()?;

    if bin_location == bin_current {
        //
        // We can't upgrade ourselves, we have to respawn from a temporary
        // location
        //
        info!("spawning upgrade process");
        spawn_upgrade()?;
    } else {
        self_upgrade()?;
    }

    Ok(())
}
