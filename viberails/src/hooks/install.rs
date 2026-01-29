use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, Result, anyhow};
use colored::Colorize;
use log::{info, warn};

use crate::{
    common::print_header,
    providers::{Claude, LLmProviderTrait, Providers},
};

const LABEL_WIDTH: usize = 12;

struct InstallResult {
    provider: Providers,
    result: Result<(), Error>,
}

impl fmt::Display for InstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.result {
            Ok(()) => write!(f, "{:<LABEL_WIDTH$} {}", self.provider, "[SUCCESS]".green()),
            Err(e) => write!(
                f,
                "{:<LABEL_WIDTH$} {} {}",
                self.provider,
                "[FAILURE]".red(),
                e
            ),
        }
    }
}

fn install_hooks(program: &Path) -> Vec<InstallResult> {
    info!("Installing hooks");

    let mut results = vec![];

    if let Ok(claude) = Claude::new(program) {
        let ret = claude.install("PreToolUse");

        let result = InstallResult {
            provider: Providers::ClaudeCode,
            result: ret,
        };

        results.push(result);
    }

    results
}

fn uninstall_hooks(program: &Path) -> Vec<InstallResult> {
    info!("Uninstalling hooks");

    let mut results = vec![];

    if let Ok(claude) = Claude::new(program) {
        let ret = claude.uninstall("PreToolUse");

        let result = InstallResult {
            provider: Providers::ClaudeCode,
            result: ret,
        };

        results.push(result);
    }

    results
}

fn display_results(results: &[InstallResult]) {
    print_header();
    for r in results {
        println!("{r}");
    }
}

fn install_binary(dst: &Path) -> Result<()> {
    info!("Install location {}", dst.display());

    //
    // We'll try to overwrite regardlless if it exists or not
    //
    let current_exe = env::current_exe().context("Unable to find current exe")?;

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

fn binary_location() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Unable to find home directory"))?;

    let local_bin = home.join(".local").join("bin");

    if !local_bin.exists() {
        fs::create_dir_all(&local_bin)
            .with_context(|| format!("Unable to create {}", local_bin.display()))?;
    }

    let current_exe = env::current_exe().context("Unable to find current exe")?;

    let file_name = current_exe
        .file_name()
        .ok_or_else(|| anyhow!("Unable to basename the current exe"))?;

    Ok(local_bin.join(file_name))
}

////////////////////////////////////////////////////////////////////////////////
// PIBLIC
////////////////////////////////////////////////////////////////////////////////

pub fn install() -> Result<()> {
    //
    // We also have to install ourselves on the host. We'll do like claude-code
    // and intall ourselves in ~/.local/bin/
    //
    // It's doesn't matter if it's not in the path since the hook contains
    // an absolute path
    //
    let dst = binary_location()?;
    install_binary(&dst)?;

    let results = install_hooks(&dst);

    display_results(&results);

    Ok(())
}

pub fn uninstall() -> Result<()> {
    let dst = binary_location()?;

    let results = uninstall_hooks(&dst);

    display_results(&results);
    uninstall_binary(&dst)?;

    Ok(())
}
