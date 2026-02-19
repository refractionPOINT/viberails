use std::time::Instant;

use anyhow::{Context, Result, bail};
use log::{debug, error, info};

use crate::{
    cloud::query::CloudQuery,
    common::PROJECT_NAME,
    config::Config,
    providers::{
        LLmProviderTrait, Providers, claudecode::ClaudeCode, copilot::Copilot, cursor::Cursor,
        gemini::Gemini, log_payload_structure, openclaw::OpenClaw, opencode::OpenCode,
    },
};

/// Main hook handler for providers that read from stdin.
///
/// Parameters:
///   - provider: The provider type (`ClaudeCode`, `Cursor`, etc.)
///
/// Returns: Result indicating success or failure
pub fn hook(provider: Providers) -> Result<()> {
    info!("{PROJECT_NAME} hook starting for provider: {provider}");

    let config = Config::load()?;
    debug!(
        "Config loaded: audit_tool_use={}, audit_prompts={}, fail_open={}, debug={}",
        config.user.audit_tool_use,
        config.user.audit_prompts,
        config.user.fail_open,
        config.user.debug
    );

    if !config.org.authorized() {
        debug!(
            "Organization not authorized (oid={}, url={})",
            config.org.oid, config.org.url
        );
        bail!("not authorized");
    }
    debug!("Organization authorized: oid={}", config.org.oid);

    // This'll fail if we're not authorized
    let ret = CloudQuery::new(&config, provider).context("Unable to initialize Cloud API");

    // Let the user decide to fail open if not properly configured
    let cloud = match ret {
        Ok(v) => {
            debug!("Cloud API initialized successfully");
            v
        }
        Err(e) => {
            error!("Unable to init cloud {e}");
            if config.user.fail_open {
                debug!("fail_open=true, allowing despite cloud init failure");
                return Ok(());
            }
            return Err(e);
        }
    };

    debug!("Delegating to provider-specific IO handler");
    match provider {
        Providers::ClaudeCode => ClaudeCode::new()?.io(&cloud, &config),
        Providers::Cursor => Cursor::new()?.io(&cloud, &config),
        Providers::GeminiCli => Gemini::new()?.io(&cloud, &config),
        Providers::OpenCode => OpenCode::new()?.io(&cloud, &config),
        Providers::OpenClaw => OpenClaw::new()?.io(&cloud, &config),
        Providers::Copilot => Copilot::new()?.io(&cloud, &config),
        Providers::Codex => bail!("Codex requires payload argument, use codex_hook() instead"),
    }
}

/// Codex-specific hook that receives JSON payload as a command line argument
/// (unlike other providers that read from stdin)
///
/// Parameters:
///   - payload: JSON string passed as command line argument from Codex
///
/// Returns: Result indicating success or failure
pub fn codex_hook(payload: &str) -> Result<()> {
    info!("{PROJECT_NAME} codex hook starting");
    debug!("Codex receives payload via CLI argument (not stdin)");

    let config = Config::load()?;
    debug!(
        "Config loaded: audit_prompts={}, fail_open={}",
        config.user.audit_prompts, config.user.fail_open
    );

    if !config.org.authorized() {
        debug!("Organization not authorized, failing");
        bail!("not authorized");
    }

    let ret = CloudQuery::new(&config, Providers::Codex).context("Unable to initialize Cloud API");

    let cloud = match ret {
        Ok(v) => {
            debug!("Cloud API initialized successfully");
            v
        }
        Err(e) => {
            error!("Unable to init cloud {e}");
            if config.user.fail_open {
                debug!("fail_open=true, allowing despite cloud init failure");
                return Ok(());
            }
            return Err(e);
        }
    };

    info!("Received JSON payload (length={})", payload.len());

    let value: serde_json::Value =
        serde_json::from_str(payload).context("Unable to deserialize JSON payload")?;

    // Log the raw payload structure for debugging and format discovery
    log_payload_structure(&value);

    let start = Instant::now();

    // Codex notify is for notifications only (e.g., agent-turn-complete)
    // It doesn't require a response, so we just send to cloud if audit_prompts is enabled
    if config.user.audit_prompts {
        debug!("Sending Codex notification to cloud");
        if let Err(e) = cloud.notify(value) {
            error!("Unable to notify cloud ({e})");
        } else {
            debug!("Cloud notification sent successfully");
        }
    } else {
        info!("audit_prompts disabled, skipping cloud notification");
    }

    let duration = start.elapsed().as_millis();
    info!("Codex hook completed in {duration}ms");

    Ok(())
}
