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
    tui::{MessageStyle, message_prompt, select_prompt, select_prompt_with_shortcuts, text_prompt},
    uninstall_all, uninstall_hooks, upgrade,
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
    Install {
        /// Provider IDs to install (comma-separated: claude-code,cursor,gemini-cli,codex,opencode,openclaw) or "all" for all detected
        #[arg(long, short)]
        providers: Option<String>,
    },
    /// Uninstall hooks from selected providers (keeps binary and config)
    #[command(visible_alias = "uninstall")]
    UninstallHooks,
    /// Uninstall everything: remove all hooks, binary, config, and data
    UninstallAll {
        /// Skip confirmation prompt and proceed with uninstall
        #[arg(long, short)]
        yes: bool,
    },

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

/// Prompt the user for confirmation before running uninstall-all via CLI.
/// Reads a single line from stdin and checks for "y" or "yes" (case-insensitive).
///
/// Parameters: None
///
/// Returns: `Ok(true)` if user confirmed, `Ok(false)` if declined, `Err` on IO failure.
fn confirm_uninstall_cli() -> Result<bool> {
    println!("This will permanently remove:");
    println!("  - All hooks from all providers");
    println!("  - Configuration and team settings");
    println!("  - Debug logs and data directory");
    println!("  - The viberails binary");
    println!();
    print!("Are you sure? [y/N]: ");
    io::stdout().flush().context("Failed to flush stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("Failed to read confirmation input")?;

    let answer = input.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
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
/// Returns: A tuple of (`ran_uninstall_all`, result). The bool is always valid
///   regardless of whether the action succeeded — even a *failed* uninstall-all
///   may have partially cleaned up, so the caller should skip auto-upgrade either way.
#[allow(clippy::too_many_lines)]
fn show_menu() -> (bool, Result<()>) {
    let mut ran_uninstall_all = false;
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
        .context("Failed to read menu selection");

        let selection_idx = match selection_idx {
            Ok(s) => s,
            Err(e) => return (ran_uninstall_all, Err(e)),
        };

        // Handle cancellation (Esc) - exit the loop
        let Some(idx) = selection_idx else {
            return (ran_uninstall_all, Ok(()));
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
                if let Err(e) = install(None) {
                    return (ran_uninstall_all, Err(e));
                }
                open_team_dashboard();
                return (ran_uninstall_all, Ok(()));
            }
            Some(MenuAction::JoinTeam) => {
                let url = match text_prompt::<fn(&str) -> viberails::tui::ValidationResult>(
                    "Enter the team URL:",
                    Some("Enter to submit, Esc to cancel"),
                    None,
                )
                .context("Failed to read team URL")
                {
                    Ok(u) => u,
                    Err(e) => return (ran_uninstall_all, Err(e)),
                };

                let Some(url) = url else {
                    continue; // User cancelled, show menu again
                };

                let args = JoinTeamArgs { url };
                if let Err(e) = join_team(&args) {
                    eprintln!("Failed to join team: {e}");
                    wait_for_keypress();
                    continue;
                }
                if let Err(e) = install(None) {
                    return (ran_uninstall_all, Err(e));
                }
                open_team_dashboard();
                return (ran_uninstall_all, Ok(()));
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
                if let Err(e) = install(None) {
                    return (ran_uninstall_all, Err(e));
                }
                return (ran_uninstall_all, Ok(()));
            }
            Some(MenuAction::UninstallHooks) => {
                let r = uninstall_hooks();
                wait_for_keypress();
                r
            }
            Some(MenuAction::UninstallAll) => {
                // Confirm before proceeding — uninstall-all is destructive and irreversible
                let confirm = select_prompt(
                    "Confirm Uninstall",
                    vec!["Yes, uninstall everything", "Cancel"],
                    Some("This will permanently remove all hooks, configuration, data, and the binary. This cannot be undone."),
                )
                .context("Failed to read confirmation");

                match confirm {
                    Ok(Some(0)) => {
                        // User confirmed — proceed with uninstall
                        let r = uninstall_all();
                        ran_uninstall_all = true;
                        wait_for_keypress();
                        r
                    }
                    Ok(_) | Err(_) => {
                        // User cancelled or pressed Esc — return to menu
                        continue;
                    }
                }
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
            Some(MenuAction::Quit) | None => return (ran_uninstall_all, Ok(())),
        };

        // If an action failed, propagate the error while preserving the flag
        if let Err(e) = result {
            return (ran_uninstall_all, Err(e));
        }
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

    // Skip auto-upgrade after full uninstall — it would re-download the binary
    // and recreate config files we just removed.
    // Tracks both CLI (Command::UninstallAll) and TUI menu (show_menu returns true).
    let mut is_uninstall_all = matches!(args.command, Some(Command::UninstallAll { .. }));

    if is_callback {
        init_callback_logging()?;
    } else {
        init_logging(args.verbose)?;
    }

    let ret = match args.command {
        None => {
            // show_menu returns (bool, Result) — the flag is always valid even
            // when the action failed, since a failed uninstall-all may have
            // partially cleaned up and we must not re-download via upgrade poll.
            let (did_uninstall_all, menu_result) = show_menu();
            if did_uninstall_all {
                is_uninstall_all = true;
            }
            menu_result
        }
        Some(Command::Install { providers }) => install(providers.as_deref()),
        Some(Command::UninstallHooks) => uninstall_hooks(),
        Some(Command::UninstallAll { yes }) => {
            if !yes && !confirm_uninstall_cli()? {
                println!("Aborted.");
                return Ok(());
            }
            uninstall_all()
        }
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
    // Skip after uninstall-all - would undo the cleanup by re-downloading the binary.
    //
    if !is_callback
        && !is_uninstall_all
        && is_auto_upgrade_enabled()
        && let Err(e) = poll_upgrade()
    {
        warn!("upgrade failure: {e}");
    }

    ret
}
