mod cloud;
mod common;
mod config;
mod hooks;
mod logging;
mod providers;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use crate::{
    common::PROJECT_NAME,
    config::{ConfigureArgs, configure, show_configuration},
    hooks::{hook, install, list, uninstall},
    logging::Logging,
};

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct UserArgs {
    #[command(subcommand)]
    command: Option<Command>,

    /// Verbose
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Auth
    Auth,

    /// Configure
    Configure(Box<ConfigureArgs>),

    /// Show Config
    ShowConfiguration,

    /// Install hooks
    Install,
    /// Uninstall hooks
    Uninstall,

    /// List Hooks
    #[command(alias = "ls")]
    List,
}

fn init_logging(verbose: bool) -> Result<()> {
    if verbose {
        Logging::new().start()
    } else {
        let file_name = format!("{PROJECT_NAME}.log");
        Logging::new().with_file(file_name).start()
    }
}

fn main() -> Result<()> {
    let args = UserArgs::parse();

    init_logging(args.verbose)?;

    match args.command {
        Some(Command::Install) => install(),
        Some(Command::Uninstall) => uninstall(),
        Some(Command::List) => list(),
        Some(Command::Configure(a)) => configure(&a),
        Some(Command::ShowConfiguration) => show_configuration(),
        Some(Command::Auth) => bail!("Not Implemented"),
        _ => hook(),
    }
}
