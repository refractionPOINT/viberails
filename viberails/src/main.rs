mod cloud;
mod common;
mod config;
mod hooks;
mod logging;
mod providers;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use crate::{
    config::{ConfigureArgs, configure},
    hooks::{hook, install, list, uninstall},
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
    Configure(ConfigureArgs),

    /// Install hooks
    Install,
    /// Uninstall hooks
    Uninstall,

    /// List Hooks
    List,
}

fn main() -> Result<()> {
    let args = UserArgs::parse();

    match args.command {
        Some(Command::Install) => install(),
        Some(Command::Uninstall) => uninstall(),
        Some(Command::List) => list(),
        Some(Command::Configure(a)) => configure(&a),
        Some(Command::Auth) => bail!("Not Implemented"),
        None => hook(),
    }
}
