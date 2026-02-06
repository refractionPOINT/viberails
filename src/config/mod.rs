mod loader;
mod upgrade_config;

pub use loader::{
    Config, ConfigureArgs, JoinTeamArgs, LcOrg, clean_debug_logs, configure, get_debug_log_path,
    is_authorized, is_debug_mode_enabled, join_team, set_debug_mode, show_configuration,
};
pub use upgrade_config::{is_upgrade_check_due, touch_last_upgrade_check};

#[cfg(test)]
mod loader_tests;
