use std::io::{self, Write};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use log::warn;

use viberails::{
    ConfigureArgs, JoinTeamArgs, Logging, LoginArgs, MenuAction, PROJECT_NAME, PROJECT_VERSION,
    Providers, UpgradeResult, clean_debug_logs, codex_hook, configure, get_debug_log_path,
    get_menu_options, hook, install, is_authorized, is_auto_upgrade_enabled, is_browser_available,
    join_team, list, login, open_browser, poll_upgrade, set_debug_mode, show_configuration,
    tui::{MessageStyle, message_prompt, select_prompt_with_shortcuts, text_prompt},
    uninstall, uninstall_hooks, upgrade,
};

#[derive(Parser)]
#[command(version =  PROJECT_VERSION, about, long_about = None)]
pub struct UserArgs {
    #[command(subcommand)]
    command: Option<Command>,

    /// Verbose
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize team configuration via OAuth
    #[command(visible_alias = "init")]
    InitTeam(LoginArgs),

    /// Join an existing team using a team URL
    #[command(visible_alias = "join")]
    JoinTeam(JoinTeamArgs),

    /// Configure audit and behavior settings
    #[command(visible_alias = "config")]
    Configure(Box<ConfigureArgs>),

    /// Show Config
    #[command(visible_alias = "show-config")]
    ShowConfiguration,

    /// Install hooks
    Install,
    /// Uninstall hooks
    Uninstall,

    /// List Hooks
    #[command(visible_alias = "ls")]
    List,

    /// Upgrade to the latest version
    Upgrade {
        /// Force upgrade even if already on latest version, skip version check
        #[arg(long, short)]
        force: bool,
    },

    /// Enable or disable debug mode for troubleshooting hooks.
    /// Note: Debug logs accumulate over time (one file per hook invocation).
    /// Use 'viberails debug-clean' to remove old logs.
    Debug {
        /// Disable debug mode
        #[arg(long, short)]
        disable: bool,
    },

    /// Clean up debug log files to free disk space
    #[command(visible_alias = "clean-debug")]
    DebugClean,

    // Provider callback commands (internal - used by hooks)
    /// Claude Code callback
    #[command(visible_alias = "cc", hide = true)]
    ClaudeCallback,

    /// Cursor callback
    #[command(hide = true)]
    CursorCallback,

    /// Gemini CLI callback
    #[command(hide = true)]
    GeminiCallback,

    /// `OpenAI` Codex callback
    #[command(hide = true)]
    CodexCallback {
        /// JSON payload from Codex (passed as command line argument)
        payload: String,
    },

    /// `OpenCode` callback
    #[command(hide = true)]
    OpencodeCallback,

    /// `OpenClaw` callback
    #[command(hide = true)]
    OpenclawCallback,
}

fn init_logging(verbose: bool) -> Result<()> {
    // Check debug mode from config (lightweight check, no permission fixes)
    let debug_mode = verbose || viberails::config::is_debug_mode_enabled();

    if verbose {
        // Verbose flag: log to stderr
        Logging::new().with_debug_mode(debug_mode).start()
    } else {
        // Normal: log to file
        let file_name = format!("{PROJECT_NAME}.log");
        Logging::new()
            .with_file(file_name)
            .with_debug_mode(debug_mode)
            .start()
    }
}

/// Wait for user to press any key before continuing.
/// Used to let users see output before returning to the menu.
///
/// Parameters: None
///
/// Returns: Nothing
fn wait_for_keypress() {
    print!("\nPress any key to continue...");
    let _ = io::stdout().flush();

    // Enable raw mode to capture single keypress
    if enable_raw_mode().is_ok() {
        loop {
            if let Ok(Event::Key(key)) = event::read()
                && key.kind == KeyEventKind::Press
            {
                break;
            }
        }
        let _ = disable_raw_mode();
    }
    println!();
}

/// Open the team dashboard in the default browser after team setup.
/// Best-effort: silently continues if the browser can't be opened.
fn open_team_dashboard() {
    if let Ok(config) = viberails::config::Config::load()
        && !config.org.oid.is_empty()
        && is_browser_available()
    {
        let team_url = format!(
            "https://app.viberails.io/viberails/teams/{}",
            config.org.oid
        );
        let _ = open_browser(&team_url);
    }
}

/// Initialize logging for callback commands with debug mode support.
/// Checks config for debug flag and enables verbose logging if set.
///
/// Uses a lightweight check for debug mode first (without triggering permission
/// fixes), then initializes logging, then loads full config (which may fix
/// permissions and emit debug messages).
///
/// Parameters: None
///
/// Returns: Result indicating success or failure
fn init_callback_logging() -> Result<()> {
    // Check debug mode without triggering permission fixes (avoids losing debug messages)
    let debug_mode = viberails::config::is_debug_mode_enabled();

    let file_name = format!("{PROJECT_NAME}.log");
    Logging::new()
        .with_file(file_name)
        .with_debug_mode(debug_mode)
        .start()?;

    // Now load config properly - this may fix permissions and emit debug messages
    // which will be captured by the now-initialized logger
    let _ = viberails::config::Config::load();

    Ok(())
}

/// Display the interactive menu and execute the selected action in a loop.
///
/// Parameters: None
///
/// Returns: Result indicating success or failure
fn show_menu() -> Result<()> {
    loop {
        let options = get_menu_options();
        let items: Vec<(&str, Option<char>)> =
            options.iter().map(|o| (o.label, o.shortcut)).collect();

        let selection_idx = select_prompt_with_shortcuts(
            "What would you like to do?",
            items,
            Some("↑↓/keys navigate, Enter select, Esc cancel"),
            Some(PROJECT_VERSION),
        )
        .context("Failed to read menu selection")?;

        // Handle cancellation (Esc) - exit the loop
        let Some(idx) = selection_idx else {
            return Ok(());
        };

        // Get the action for the selected index
        let action = options.get(idx).map(|o| o.action);

        let result = match action {
            Some(MenuAction::InitializeTeam) => {
                let args = LoginArgs {
                    no_browser: false,
                    existing_org: None,
                };
                if let Err(e) = login(&args) {
                    eprintln!("Login failed: {e}");
                    wait_for_keypress();
                    continue;
                }
                install()?;
                open_team_dashboard();
                return Ok(()); // Exit after successful installation
            }
            Some(MenuAction::JoinTeam) => {
                let url = text_prompt::<fn(&str) -> viberails::tui::ValidationResult>(
                    "Enter the team URL:",
                    Some("Enter to submit, Esc to cancel"),
                    None,
                )
                .context("Failed to read team URL")?;

                let Some(url) = url else {
                    continue; // User cancelled, show menu again
                };

                let args = JoinTeamArgs { url };
                if let Err(e) = join_team(&args) {
                    eprintln!("Failed to join team: {e}");
                    wait_for_keypress();
                    continue;
                }
                install()?;
                open_team_dashboard();
                return Ok(()); // Exit after successful installation
            }
            Some(MenuAction::InstallHooks) => {
                if !is_authorized() {
                    let _ = message_prompt(
                        " Not Logged In ",
                        "Please initialize or join a team first",
                        MessageStyle::Error,
                    );
                    continue;
                }
                install()?;
                return Ok(()); // Exit after successful installation
            }
            Some(MenuAction::UninstallHooks) => {
                let r = uninstall_hooks();
                wait_for_keypress();
                r
            }
            Some(MenuAction::UninstallFully) => {
                let r = uninstall();
                wait_for_keypress();
                r
            }
            Some(MenuAction::ListHooks) => {
                list();
                wait_for_keypress();
                Ok(())
            }
            Some(MenuAction::ShowConfiguration) => {
                let r = show_configuration();
                wait_for_keypress();
                r
            }
            Some(MenuAction::Quit) | None => return Ok(()),
        };

        // If an action failed, propagate the error
        result?;
    }
}

fn main() -> Result<()> {
    let args = UserArgs::parse();

    // Use callback-specific logging for provider callbacks (supports debug mode)
    // Use regular logging for other commands
    let is_callback = matches!(
        args.command,
        Some(
            Command::ClaudeCallback
                | Command::CursorCallback
                | Command::GeminiCallback
                | Command::CodexCallback { .. }
                | Command::OpencodeCallback
                | Command::OpenclawCallback
        )
    );

    if is_callback {
        init_callback_logging()?;
    } else {
        init_logging(args.verbose)?;
    }

    let ret = match args.command {
        None => show_menu(),
        Some(Command::Install) => install(),
        Some(Command::Uninstall) => uninstall(),
        Some(Command::List) => {
            list();
            Ok(())
        }
        Some(Command::ShowConfiguration) => show_configuration(),
        Some(Command::Configure(args)) => configure(&args),
        Some(Command::InitTeam(args)) => login(&args),
        Some(Command::JoinTeam(args)) => join_team(&args),
        Some(Command::Upgrade { force }) => {
            // CLI upgrade: verbose output to show user what's happening
            match upgrade(force, true)? {
                UpgradeResult::AlreadyLatest { version } => {
                    println!("Already on latest version ({version}).");
                }
                UpgradeResult::Upgraded { from, to } => {
                    println!("Successfully upgraded from {from} to {to}.");
                }
                UpgradeResult::Reinstalled { version } => {
                    println!("Successfully reinstalled version {version}.");
                }
                UpgradeResult::Spawned => {
                    println!("Upgrade process started.");
                }
                UpgradeResult::InProgress => {
                    println!("Another upgrade is already in progress.");
                }
            }
            Ok(())
        }
        Some(Command::Debug { disable }) => {
            set_debug_mode(!disable)?;
            if !disable {
                println!();
                println!("Debug log location: {}", get_debug_log_path()?.display());
                println!();
                println!("Note: Debug logs accumulate over time. Run 'viberails debug-clean' to remove old logs.");
            }
            Ok(())
        }
        Some(Command::DebugClean) => {
            clean_debug_logs()?;
            Ok(())
        }

        // Provider callbacks
        Some(Command::ClaudeCallback) => hook(Providers::ClaudeCode),
        Some(Command::CursorCallback) => hook(Providers::Cursor),
        Some(Command::GeminiCallback) => hook(Providers::GeminiCli),
        Some(Command::CodexCallback { payload }) => codex_hook(&payload),
        Some(Command::OpencodeCallback) => hook(Providers::OpenCode),
        Some(Command::OpenclawCallback) => hook(Providers::OpenClaw),
    };

    //
    // This'll try to upgrade every x hours on exit (if enabled).
    // Skip for hook callbacks - they must exit quickly to avoid blocking
    // the AI tool (e.g., Claude Code waits for the hook process to exit).
    //
    if !is_callback
        && is_auto_upgrade_enabled()
        && let Err(e) = poll_upgrade()
    {
        warn!("upgrade failure: {e}");
    }

    ret
}
