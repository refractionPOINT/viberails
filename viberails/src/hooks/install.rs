use anyhow::{Context, Result};
use log::info;

use crate::{
    logging::init_logging,
    providers::{Claude, LLmProviderTrait, Providers},
};

#[derive(clap::Args)]
pub struct InstallArgs {
    /// Providers
    #[arg(short, long)]
    provider: Providers,
}

#[derive(clap::Args)]
pub struct UninstallArgs {
    /// Providers
    #[arg(short, long)]
    provider: Providers,

    /// Clear and clear everything
    #[arg(short, long)]
    clear: bool,
}

fn install_hooks(args: &InstallArgs) -> Result<()> {
    info!("Installing hooks for {}", args.provider);

    match args.provider {
        Providers::ClaudeCode => {
            let claude = Claude::new()?;
            claude.install("PreToolUse")
        }
    }
}

fn uninstall_hooks(args: &UninstallArgs) -> Result<()> {
    info!("Installing hooks for {}", args.provider);

    match args.provider {
        Providers::ClaudeCode => {
            let claude = Claude::new()?;
            claude.uninstall("PreToolUse")
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// PIBLIC
////////////////////////////////////////////////////////////////////////////////
pub fn install(args: &InstallArgs) -> Result<()> {
    init_logging(Some("install.log")).context("Unable to initialize logging")?;
    install_hooks(args)
}

pub fn uninstall(args: &UninstallArgs) -> Result<()> {
    init_logging(Some("uninstall.log")).context("Unable to initialize logging")?;
    uninstall_hooks(args)
}
