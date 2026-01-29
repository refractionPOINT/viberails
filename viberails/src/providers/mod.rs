use anyhow::Result;

pub mod claude;
pub use claude::Claude;
use derive_more::Display;

#[cfg(test)]
mod claude_tests;

#[derive(Clone, clap::ValueEnum, Display)]
pub enum Providers {
    ClaudeCode,
}

pub trait LLmProviderTrait {
    fn install(&self, hook_type: &str) -> Result<()>;
    fn uninstall(&self, hook_type: &str) -> Result<()>;
    //fn config_file(&self) -> &Path;
}
