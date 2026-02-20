use std::time::Instant;

use anyhow::{Context, Result, bail};
use log::{debug, error, info};

use crate::{
    cloud::{CloudTrait, LcCloud, lc_socket::LcSocket},
    common::PROJECT_NAME,
    config::Config,
    providers::{
        LLmProviderTrait, Providers, claudecode::ClaudeCode, cursor::Cursor, gemini::Gemini,
        log_payload_structure, openclaw::OpenClaw, opencode::OpenCode,
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

    let mut clouds: Vec<Box<dyn CloudTrait>> = Vec::new();

    if let Ok(socket) = LcSocket::new() {
        info!("Using lc_socket");
        clouds.push(Box::new(socket));
    }

    if config.org.authorized() {
        debug!("Organization authorized: oid={}", config.org.oid);

        let ret = LcCloud::new(config.clone(), provider).context("Unable to initialize Cloud API");

        // Let the user decide to fail open if not properly configured
        match ret {
            Ok(v) => {
                debug!("Cloud API initialized successfully");
                clouds.push(Box::new(v));
            }
            Err(e) => {
                error!("Unable to init cloud {e}");
                if config.user.fail_open {
                    debug!("fail_open=true, allowing despite cloud init failure");
                }
                return Err(e);
            }
        }
    } else {
        debug!(
            "Organization not authorized (oid={}, url={})",
            config.org.oid, config.org.url
        );
    }

    info!("clound providers: {}", clouds.len());

    //
    // fast path, no configuration found, just return success
    //
    if clouds.is_empty() {
        return Ok(());
    }

    match provider {
        Providers::ClaudeCode => ClaudeCode::new()?.io(&clouds, &config),
        Providers::Cursor => Cursor::new()?.io(&clouds, &config),
        Providers::GeminiCli => Gemini::new()?.io(&clouds, &config),
        Providers::OpenCode => OpenCode::new()?.io(&clouds, &config),
        Providers::OpenClaw => OpenClaw::new()?.io(&clouds, &config),
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

    let ret =
        LcCloud::new(config.clone(), Providers::Codex).context("Unable to initialize Cloud API");

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
        if let Err(e) = cloud.notify(&value) {
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
