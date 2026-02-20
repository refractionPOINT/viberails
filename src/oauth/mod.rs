pub mod login;
pub mod primer_rules;

pub use auth::{LoginArgs, OAuthProvider, authorize, is_browser_available, open_browser};

pub mod auth;
