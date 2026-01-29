mod cloud;
mod common;
mod hooks;
mod logging;
mod providers;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::hooks::{InstallArgs, UninstallArgs, hook, install, list, uninstall};

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
    /// Install hooks
    Install(InstallArgs),
    /// Uninstall hooks
    Uninstall(UninstallArgs),

    /// List Hooks
    List,
}

fn main() -> Result<()> {
    let args = UserArgs::parse();

    match args.command {
        Some(Command::Install(i)) => install(&i),
        Some(Command::Uninstall(u)) => uninstall(&u),
        Some(Command::List) => list(),
        None => hook(),
    }
}
