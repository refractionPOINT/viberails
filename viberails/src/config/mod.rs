mod loader;
pub use loader::{Config, ConfigureArgs, configure, show_configuration, uninstall_config};

#[cfg(test)]
mod loader_tests;
