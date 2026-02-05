pub mod login;
pub mod primer_rules;

#[allow(unused_imports)] // OAuthTokens is part of the public API
pub use auth::{LoginArgs, OAuthProvider, OAuthTokens, authorize, is_browser_available, open_browser};

pub mod auth;
