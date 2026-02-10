use std::{
    fmt::Display,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Stdout, Write},
    path::Path,
    time::Instant,
};

use anyhow::Result;

pub mod claudecode;
pub mod codex;
pub mod cursor;
pub mod discovery;
pub mod gemini;
pub mod openclaw;
pub mod opencode;
pub mod registry;
pub mod selector;

// Re-exports for public API (may be used by external code or future extensions)
#[allow(unused_imports)]
pub use discovery::{DiscoveryResult, ProviderDiscovery, ProviderFactory};
use log::{debug, error, info, warn};
pub use registry::ProviderRegistry;
pub use selector::{select_providers, select_providers_for_uninstall};

use crate::{
    cloud::query::{CloudQuery, CloudVerdict},
    common::PROJECT_NAME,
    config::Config,
};
use anyhow::Context;
use derive_more::Display;
use serde::Serialize;
use serde_json::Value;

#[cfg(test)]
mod tests;

// Keys that indicate a tool use event from various providers:
// - tool_input, tool_name, tool_use_id: Claude Code format
// - toolName: OpenClaw before_tool_call hook format (PR #6264)
//   Note: "params" and "parameters" are not included as they're too generic
const TOOL_HINTS: &[&str] = &["tool_input", "tool_name", "tool_use_id", "toolName"];

// Hook event names that indicate the agent has finished responding
// These hooks provide transcript_path but not the actual response content
const STOP_HOOK_EVENTS: &[&str] = &["Stop", "afterAgentResponse", "AfterAgent", "session.idle"];

// Keys that might contain prompt/message text across various providers
const PROMPT_HINTS: &[&str] = &[
    "content",
    "prompt",
    "message",
    "text",
    "user_prompt",
    "input",
    "query",
];

/// Extract the last assistant response text from a transcript JSONL file.
///
/// Parameters:
///   - `transcript_path`: Path to the JSONL transcript file
///
/// Returns: The concatenated text content from the last assistant message, or None
pub(crate) fn extract_last_response_from_transcript(transcript_path: &Path) -> Option<String> {
    let file = File::open(transcript_path).ok()?;
    let reader = BufReader::new(file);

    let mut last_assistant_content: Option<Vec<Value>> = None;

    for line in reader.lines().map_while(Result::ok) {
        if let Ok(entry) = serde_json::from_str::<Value>(&line) {
            // Check if this is an assistant message
            if entry.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                // Extract the content array from message.content
                if let Some(content) = entry
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
                    last_assistant_content = Some(content.clone());
                }
            }
        }
    }

    // Extract text from the content array
    let content = last_assistant_content?;
    let text_parts: Vec<&str> = content
        .iter()
        .filter_map(|item| {
            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                item.get("text").and_then(|t| t.as_str())
            } else {
                None
            }
        })
        .collect();

    if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join("\n"))
    }
}

/// Log the payload structure for debugging and format discovery.
/// This helps understand provider-specific payload formats for normalization.
/// Uses debug level for detailed payload capture, info level for summaries.
///
/// Parameters:
///   - value: The JSON payload received from the provider hook
///
/// Returns: Nothing, logs at debug/info levels
pub fn log_payload_structure(value: &Value) {
    // Log the full payload at debug level for detailed analysis
    if let Ok(pretty) = serde_json::to_string_pretty(value) {
        debug!("PAYLOAD_CAPTURE (full JSON):\n{pretty}");
    }

    // Log top-level keys for quick overview (info level)
    if let Some(obj) = value.as_object() {
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        info!("PAYLOAD_KEYS: {keys:?}");

        // Check for tool-related keys
        let tool_keys: Vec<&str> = TOOL_HINTS
            .iter()
            .filter(|k| obj.contains_key(**k))
            .copied()
            .collect();
        if tool_keys.is_empty() {
            info!("PAYLOAD_TYPE: prompt_use");
        } else {
            info!("PAYLOAD_TYPE: tool_use (matched keys: {tool_keys:?})");

            // Log tool-specific details at debug level
            if let Some(tool_name) = obj.get("tool_name").or_else(|| obj.get("toolName")) {
                debug!("TOOL_NAME: {tool_name}");
            }
            if let Some(tool_input) = obj.get("tool_input").or_else(|| obj.get("params"))
                && let Ok(input_str) = serde_json::to_string(tool_input)
            {
                // Use char-safe truncation to avoid panic on multi-byte UTF-8
                let truncated = if input_str.chars().count() > 500 {
                    let prefix: String = input_str.chars().take(500).collect();
                    format!("{prefix}... [truncated]")
                } else {
                    input_str
                };
                debug!("TOOL_INPUT: {truncated}");
            }
        }

        // Check for prompt-related keys and extract potential prompt text
        for hint in PROMPT_HINTS {
            if let Some(val) = obj.get(*hint)
                && let Some(text) = val.as_str()
            {
                // Use char-safe truncation to avoid panic on multi-byte UTF-8
                let char_count = text.chars().count();
                let truncated = if char_count > 200 {
                    let prefix: String = text.chars().take(200).collect();
                    format!("{prefix}... [truncated, total {char_count} chars]")
                } else {
                    text.to_string()
                };
                debug!("PAYLOAD_PROMPT_FIELD: {hint} = \"{truncated}\"");
            }
        }

        // Log session information if present
        if let Some(session_id) = obj.get("session_id") {
            debug!("SESSION_ID: {session_id}");
        }
    }
}

/// Enum representing all supported providers.
/// Used for callback command routing.
#[derive(Clone, Copy, clap::ValueEnum, Display, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Providers {
    ClaudeCode,
    Cursor,
    GeminiCli,
    Codex,
    OpenCode,
    OpenClaw,
}

#[derive(Serialize, Display, Clone)]
#[allow(dead_code)]
#[serde(rename_all = "lowercase")]
pub enum HookDecision {
    Block,
    Approve,
}

#[derive(Serialize)]
pub(crate) struct HookAnswer {
    decision: HookDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl HookAnswer {
    pub(crate) fn approve() -> Self {
        Self {
            decision: HookDecision::Approve,
            reason: None,
        }
    }

    pub(crate) fn block(reason: String) -> Self {
        Self {
            decision: HookDecision::Block,
            reason: Some(reason),
        }
    }
}

pub struct HookEntry {
    pub hook_type: String,
    pub matcher: String,
    pub command: String,
}

pub trait LLmProviderTrait: Display {
    fn install(&self, hook_type: &str) -> Result<()>;
    fn uninstall(&self, hook_type: &str) -> Result<()>;
    fn list(&self) -> Result<Vec<HookEntry>>;

    fn write_answer(&self, writer: &mut BufWriter<Stdout>, answer: HookAnswer) -> Result<()> {
        match answer.decision {
            HookDecision::Approve => {
                // Don't write anything to stdout on approve.
                // Exit 0 with no output means "allow / no opinion" across all providers
                // (Claude Code, Cursor, Gemini CLI, etc.), which lets the normal
                // permission system of the AI tool continue operating as before.
                // Writing {"decision":"approve"} would actively bypass the permission
                // system (e.g. in Claude Code it maps to permissionDecision:"allow").
                info!("decision: approve (no output, exit 0)");
                Ok(())
            }
            HookDecision::Block => {
                let resp_string = serde_json::to_string(&answer)
                    .context("Failed to serialize hook response")?;

                info!("decision json: {resp_string}");

                writer
                    .write_all(resp_string.as_bytes())
                    .context("Failed to write hook response to stdout")?;
                writer.flush().context("Failed to flush hook response")?;

                Ok(())
            }
        }
    }

    fn authorize_tool(&self, cloud: &CloudQuery, config: &Config, value: Value) -> HookAnswer {
        //
        // Do we fail-open?
        //
        match cloud.authorize(value) {
            Ok(CloudVerdict::Allow) => HookAnswer::approve(),
            Ok(CloudVerdict::Deny(r)) => {
                warn!("Deny reason: {r}");
                HookAnswer::block(r)
            }
            Err(e) => {
                error!("cloud failed ({e})");

                if config.user.fail_open {
                    HookAnswer::approve()
                } else {
                    let msg = format!("{PROJECT_NAME} cloud failure ({e})");
                    HookAnswer::block(msg)
                }
            }
        }
    }

    fn is_tool_use(&self, value: &Value) -> bool {
        for hint in TOOL_HINTS {
            if value.get(hint).is_some() {
                return true;
            }
        }

        false
    }

    /// Check if this is a Stop/response completion event and enrich it with the response.
    /// For Stop events, Claude Code only provides metadata (`transcript_path`, `session_id`, etc.)
    /// but not the actual response. This method reads the transcript and adds the response.
    /// Other platforms (Cursor, Gemini) may already include the response in the payload.
    fn enrich_stop_event(&self, value: Value) -> Value {
        let mut value = value;
        // Check if this is a stop event by looking at hook_event_name
        let hook_event = value
            .get("hook_event_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !STOP_HOOK_EVENTS.contains(&hook_event) {
            return value;
        }

        info!("Detected stop event: {hook_event}");

        // Check if response is already in the payload (some platforms include it directly)
        if value.get("assistant_response").is_some() {
            info!("Response already present in payload");
            return value;
        }

        // Try to get the transcript path - not all platforms provide it
        let Some(transcript_path) = value.get("transcript_path").and_then(|v| v.as_str()) else {
            // No transcript path and no response - nothing we can do, but this is expected
            // for platforms like Cursor/Gemini that may handle responses differently
            return value;
        };

        // Extract the last response from the transcript
        let path = Path::new(transcript_path);
        if let Some(response) = extract_last_response_from_transcript(path) {
            info!(
                "Extracted response from transcript ({} chars)",
                response.len()
            );

            // Add the response to the payload
            if let Some(obj) = value.as_object_mut() {
                obj.insert("assistant_response".to_string(), Value::String(response));
            }
        }

        value
    }
    fn io(&self, cloud: &CloudQuery, config: &Config) -> Result<()> {
        //
        // This'll fail if we're not authorized
        //

        let stdin = std::io::stdin();
        let stdout = std::io::stdout();

        let mut reader = BufReader::new(stdin);
        let mut writer = BufWriter::new(stdout);

        let mut line = String::new();

        debug!("Hook waiting for input on stdin");

        // that's a fatal error
        let len = reader
            .read_line(&mut line)
            .context("Unable to read from stdin")?;

        debug!("Received {len} bytes from stdin");

        if 0 == len {
            // that's still successful, out input just got closed
            warn!("EOF received, exiting hook");
            return Ok(());
        }

        let value: Value = serde_json::from_str(&line).context("Unable to deserialize")?;

        // Log the raw payload structure for debugging and format discovery
        // This helps understand provider-specific payload formats
        log_payload_structure(&value);

        let start = Instant::now();

        if self.is_tool_use(&value) {
            debug!("Processing as TOOL_USE event");
            debug!(
                "Config: audit_tool_use={}, fail_open={}",
                config.user.audit_tool_use, config.user.fail_open
            );

            // D&R Path - only call cloud if audit_tool_use is enabled
            let answer = if config.user.audit_tool_use {
                debug!("Sending tool_use to cloud for authorization");
                self.authorize_tool(cloud, config, value)
            } else {
                info!("audit_tool_use disabled, approving locally");
                HookAnswer::approve()
            };

            info!("Decision={}", answer.decision);
            debug!("Writing response to stdout");
            self.write_answer(&mut writer, answer)?;
        } else {
            debug!("Processing as PROMPT_USE event (notification only)");
            debug!("Config: audit_prompts={}", config.user.audit_prompts);

            // Notify path - only call cloud if audit_prompts is enabled
            if config.user.audit_prompts {
                debug!("Sending prompt to cloud for audit logging");

                // Check if this is a Stop/response completion event
                // If so, enrich the payload with the actual response from the transcript
                let enriched_value = self.enrich_stop_event(value);

                if let Err(e) = cloud.notify(enriched_value) {
                    error!("Unable to notify cloud ({e})");
                } else {
                    debug!("Cloud notification sent successfully");
                }
            } else {
                info!("audit_prompts disabled, skipping cloud notification");
            }

            // Always write an approve response for non-tool-use events
            // The AI tool may be waiting for a response on stdout
            debug!("Writing approve response to stdout (prompts always approved)");
            self.write_answer(&mut writer, HookAnswer::approve())?;
        }

        let duration = start.elapsed().as_millis();

        info!("Hook completed in {duration}ms");

        Ok(())
    }
}
