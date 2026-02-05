mod loader;
pub use loader::{
    Config, ConfigureArgs, JoinTeamArgs, LcOrg, clean_debug_logs, configure, get_debug_log_path,
    is_authorized, is_auto_upgrade_enabled, is_debug_mode_enabled, join_team, set_debug_mode,
    show_configuration,
};

#[cfg(test)]
mod loader_tests;
