mod cloud;
mod common;
pub mod config;
mod default;
pub mod edr;
mod hooks;
mod logging;
mod oauth;
mod providers;
pub mod tui;
mod upgrade;

pub use common::{PROJECT_NAME, PROJECT_VERSION};
pub use config::{
    ConfigureArgs, JoinTeamArgs, clean_debug_logs, configure, get_debug_log_path, is_authorized,
    is_auto_upgrade_enabled, join_team, set_debug_mode, show_configuration,
};
pub use hooks::{codex_hook, hook, install, list, uninstall, uninstall_hooks};
pub use logging::Logging;
pub use oauth::{LoginArgs, is_browser_available, login::login, open_browser};
pub use providers::Providers;
pub use upgrade::{UpgradeResult, poll_upgrade, upgrade};

/// Menu option for the interactive menu
pub struct MenuOption {
    pub label: &'static str,
    pub action: MenuAction,
    pub shortcut: Option<char>,
}

/// Actions that can be performed from the interactive menu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    InitializeTeam,
    JoinTeam,
    InstallHooks,
    UninstallHooks,
    UninstallFully,
    ListHooks,
    ShowConfiguration,
    Quit,
}

/// Returns all available menu options
#[must_use]
pub fn get_menu_options() -> Vec<MenuOption> {
    vec![
        MenuOption {
            label: "Initialize Team",
            action: MenuAction::InitializeTeam,
            shortcut: Some('i'),
        },
        MenuOption {
            label: "Join Team",
            action: MenuAction::JoinTeam,
            shortcut: Some('j'),
        },
        MenuOption {
            label: "Install Hooks",
            action: MenuAction::InstallHooks,
            shortcut: Some('h'),
        },
        MenuOption {
            label: "Remove Hooks",
            action: MenuAction::UninstallHooks,
            shortcut: Some('u'),
        },
        MenuOption {
            label: "List Hooks",
            action: MenuAction::ListHooks,
            shortcut: Some('l'),
        },
        MenuOption {
            label: "Show Configuration",
            action: MenuAction::ShowConfiguration,
            shortcut: Some('c'),
        },
        MenuOption {
            label: "Uninstall",
            action: MenuAction::UninstallFully,
            shortcut: Some('f'),
        },
        MenuOption {
            label: "Quit",
            action: MenuAction::Quit,
            shortcut: Some('q'),
        },
    ]
}
