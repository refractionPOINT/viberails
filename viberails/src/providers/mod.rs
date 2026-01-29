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

pub struct HookEntry {
    pub hook_type: String,
    pub matcher: String,
    pub command: String,
}

pub trait LLmProviderTrait {
    fn name(&self) -> &'static str;
    fn install(&self, hook_type: &str) -> Result<()>;
    fn uninstall(&self, hook_type: &str) -> Result<()>;
    fn list(&self) -> Result<Vec<HookEntry>>;
    //fn config_file(&self) -> &Path;
}
