use std::fmt;

use anyhow::{Context, Error, Result};
use colored::Colorize;
use log::info;

use crate::{
    logging::init_logging,
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

fn install_hooks() -> Vec<InstallResult> {
    info!("Installing hooks");

    let mut results = vec![];

    if let Ok(claude) = Claude::new() {
        let ret = claude.install("PreToolUse");

        let result = InstallResult {
            provider: Providers::ClaudeCode,
            result: ret,
        };

        results.push(result);
    }

    results
}

fn uninstall_hooks() -> Vec<InstallResult> {
    info!("Uninstalling hooks");

    let mut results = vec![];

    if let Ok(claude) = Claude::new() {
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
    for r in results {
        println!("{r}");
    }
}

////////////////////////////////////////////////////////////////////////////////
// PIBLIC
////////////////////////////////////////////////////////////////////////////////
pub fn install() -> Result<()> {
    init_logging(Some("install.log")).context("Unable to initialize logging")?;
    let results = install_hooks();

    display_results(&results);

    Ok(())
}

pub fn uninstall() -> Result<()> {
    init_logging(Some("uninstall.log")).context("Unable to initialize logging")?;

    let results = uninstall_hooks();

    display_results(&results);

    Ok(())
}
