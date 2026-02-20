use super::claudecode::ClaudeCodeDiscovery;
use super::codex::CodexDiscovery;
use super::copilot::CopilotDiscovery;
use super::cursor::CursorDiscovery;
use super::discovery::{DiscoveryResult, ProviderFactory};
use super::gemini::GeminiDiscovery;
use super::openclaw::OpenClawDiscovery;
use super::opencode::OpenCodeDiscovery;

/// Central registry of all known providers.
/// Manages discovery and creation of provider instances.
pub struct ProviderRegistry {
    providers: Vec<Box<dyn ProviderFactory>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    /// Create a new registry with all built-in providers registered.
    #[must_use]
    pub fn new() -> Self {
        // Register all built-in providers
        let providers: Vec<Box<dyn ProviderFactory>> = vec![
            Box::new(ClaudeCodeDiscovery),
            Box::new(CursorDiscovery),
            Box::new(GeminiDiscovery),
            Box::new(CodexDiscovery),
            Box::new(OpenCodeDiscovery),
            Box::new(OpenClawDiscovery),
            Box::new(CopilotDiscovery),
        ];

        Self { providers }
    }

    /// Discover all registered providers and return their discovery results.
    pub fn discover_all(&self) -> Vec<DiscoveryResult> {
        self.providers.iter().map(|p| p.discover()).collect()
    }

    /// Discover all registered providers and check if our hooks are installed.
    /// This is used for uninstall to determine which tools have our hooks.
    pub fn discover_all_with_hooks_check(&self) -> Vec<DiscoveryResult> {
        self.providers
            .iter()
            .map(|p| p.discover_with_hooks_check())
            .collect()
    }

    /// Get a provider factory by its ID.
    pub fn get(&self, id: &str) -> Option<&dyn ProviderFactory> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(AsRef::as_ref)
    }

    /// Get all provider factories.
    pub fn all(&self) -> impl Iterator<Item = &dyn ProviderFactory> {
        self.providers.iter().map(AsRef::as_ref)
    }
}
