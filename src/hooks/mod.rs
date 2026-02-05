mod hook;
pub use hook::{codex_hook, hook};

mod install;
#[cfg(test)]
mod install_test;
pub use install::binary_location;
pub use install::install;
pub use install::install_binary;
pub use install::uninstall_all;
pub use install::uninstall_hooks;

mod list;
pub use list::list;
