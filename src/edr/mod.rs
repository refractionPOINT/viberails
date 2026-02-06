use std::path::Path;

use anyhow::{Context, Result, bail};
use colored::Colorize;
use log::info;

use crate::{
    cloud::lc_api::create_installation_key,
    common::{PROJECT_NAME, user_agent},
    tui::select_prompt,
};

const SENSOR_DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// Returns the LC sensor download URL for the current platform.
fn sensor_download_url() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("https://downloads.limacharlie.io/sensor/linux/64"),
        ("linux", "aarch64") => Ok("https://downloads.limacharlie.io/sensor/linux/arm64"),
        ("macos", "x86_64") => Ok("https://downloads.limacharlie.io/sensor/mac/64"),
        ("macos", "aarch64") => Ok("https://downloads.limacharlie.io/sensor/mac/arm64"),
        ("windows", "x86_64") => Ok("https://downloads.limacharlie.io/sensor/windows/64"),
        ("windows", "x86") => Ok("https://downloads.limacharlie.io/sensor/windows/32"),
        (os, arch) => bail!("Unsupported platform: {os}/{arch}"),
    }
}

/// Downloads the sensor binary to `dest`.
fn download_sensor(url: &str, dest: &Path) -> Result<()> {
    info!("Downloading sensor from {url}");

    let mut fd = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(dest)
        .with_context(|| format!("Unable to open {} for writing", dest.display()))?;

    let res = minreq::get(url)
        .with_header("User-Agent", user_agent())
        .with_timeout(SENSOR_DOWNLOAD_TIMEOUT_SECS)
        .send()
        .with_context(|| format!("Sensor download from {url} failed"))?;

    if !(200..300).contains(&res.status_code) {
        bail!(
            "Sensor download returned HTTP {}: {}",
            res.status_code,
            res.as_str().unwrap_or("Unknown error")
        );
    }

    std::io::Write::write_all(&mut fd, res.as_bytes())
        .context("Failed to write sensor binary to disk")?;

    Ok(())
}

/// Makes the sensor binary executable (Unix only).
#[cfg(unix)]
fn make_sensor_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    info!("Making {} executable", path.display());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("Unable to make {} executable", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_sensor_executable(_path: &Path) -> Result<()> {
    Ok(())
}

/// Runs the sensor installer with the given installation key.
fn install_sensor(sensor_path: &Path, installation_key: &str) -> Result<()> {
    info!(
        "Installing sensor from {} with key {}",
        sensor_path.display(),
        installation_key
    );

    let output = if cfg!(target_os = "windows") {
        std::process::Command::new(sensor_path)
            .arg("-i")
            .arg(installation_key)
            .output()
            .context("Failed to run sensor installer")?
    } else {
        std::process::Command::new("sudo")
            .arg(sensor_path)
            .arg("-i")
            .arg(installation_key)
            .output()
            .context("Failed to run sensor installer (sudo)")?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Sensor installation exited with {}: {}",
            output.status,
            stderr.trim()
        );
    }

    Ok(())
}

/// Orchestrates the full EDR deployment: create key, download, install.
fn deploy_edr(jwt: &str, oid: &str) -> Result<()> {
    println!("{} Creating EDR installation key...", "→".blue());
    let key_desc = format!("{PROJECT_NAME} EDR sensor installation key");
    let installation_key =
        create_installation_key(jwt, oid, &key_desc).context("Failed to create installation key")?;
    info!("Installation key created: {installation_key}");

    let url = sensor_download_url()?;

    println!("{} Downloading EDR sensor...", "→".blue());
    let tmp_dir = tempfile::Builder::new()
        .prefix("edr_sensor_")
        .tempdir()
        .context("Unable to create temp directory for sensor download")?;

    let sensor_name = if cfg!(target_os = "windows") {
        "lc_sensor.exe"
    } else {
        "lc_sensor"
    };
    let sensor_path = tmp_dir.path().join(sensor_name);

    download_sensor(url, &sensor_path)?;
    make_sensor_executable(&sensor_path)?;

    println!("{} Installing EDR sensor (requires sudo)...", "→".blue());
    install_sensor(&sensor_path, &installation_key)?;

    println!("{} EDR sensor installed successfully", "✓".green());
    Ok(())
}

/// Offers EDR deployment to the user. Non-blocking: errors are printed but
/// never propagated so that hook installation is not affected.
pub fn offer_edr_deployment(jwt: &str, oid: &str) {
    let result = select_prompt(
        "Deploy LimaCharlie EDR agent on this machine?",
        vec!["Yes", "No"],
        Some("↑↓ navigate, Enter select, Esc cancel"),
    );

    let accepted = match result {
        Ok(Some(0)) => true, // "Yes"
        _ => false,          // "No", cancelled, or error
    };

    if !accepted {
        return;
    }

    if let Err(e) = deploy_edr(jwt, oid) {
        eprintln!(
            "{} EDR deployment failed: {e:#}",
            "⚠".yellow()
        );
        eprintln!(
            "{}",
            "  Hooks were installed successfully. You can install the EDR agent manually later."
                .dimmed()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensor_download_url_known_platforms() {
        // We can't change consts::OS/ARCH at runtime, but we can at least
        // verify the current platform returns a valid URL.
        let url = sensor_download_url();
        // On CI this will be linux/x86_64 or linux/aarch64
        if cfg!(target_os = "linux") || cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            assert!(url.is_ok(), "Current platform should be supported");
            let url = url.unwrap();
            assert!(url.starts_with("https://downloads.limacharlie.io/sensor/"));
        }
    }
}
