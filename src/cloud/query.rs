use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use derive_more::Display;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    common::{PROJECT_VERSION, PROJECT_VERSION_HASH, display_authorize_help, user_agent},
    config::Config,
    providers::Providers,
};

const CLOUD_API_TIMEOUT_SECS: u64 = 10;

/// Normalized event structure for consistent server-side D&R rules.
/// Extracts common fields from provider-specific payload formats.
///
/// This enables rules like:
/// - `event.normalized.prompt_text CONTAINS "delete production"`
/// - `event.normalized.tool_name == "Bash"`
/// - `event.normalized.tool_input.command MATCHES "rm -rf"`
#[derive(Serialize, Debug)]
pub struct NormalizedEvent {
    /// Event type: `prompt` or `tool_use`
    pub event_type: String,
    /// User prompt text (extracted from provider-specific field)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_text: Option<String>,
    /// Tool name for `tool_use` events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Tool input/parameters as JSON
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,
    /// Session/thread identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Working directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

impl NormalizedEvent {
    /// Create a normalized event from a raw provider payload.
    /// Extracts fields from various provider-specific formats.
    ///
    /// Parameters:
    ///   - data: Raw JSON payload from the provider hook
    ///   - `is_tool_use`: Whether this is a `tool_use` event (vs prompt)
    ///
    /// Returns: `NormalizedEvent` with extracted fields
    pub fn from_payload(data: &Value, is_tool_use: bool) -> Self {
        let event_type = if is_tool_use {
            "tool_use".to_string()
        } else {
            "prompt".to_string()
        };

        Self {
            event_type,
            prompt_text: extract_prompt_text(data),
            tool_name: extract_tool_name(data),
            tool_input: extract_tool_input(data),
            session_id: extract_session_id(data),
            cwd: extract_cwd(data),
        }
    }
}

/// Extract prompt text from various provider formats.
/// Checks multiple possible field names used by different providers.
///
/// Parameters:
///   - data: Raw JSON payload
///
/// Returns: Extracted prompt text if found
fn extract_prompt_text(data: &Value) -> Option<String> {
    let obj = data.as_object()?;

    // Claude Code, Cursor: "prompt"
    if let Some(val) = obj.get("prompt").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    // OpenClaw: "content" (for message_received events)
    if let Some(val) = obj.get("content").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    // Codex: "input-messages" (array of strings, take last user message)
    if let Some(arr) = obj.get("input-messages").and_then(|v| v.as_array()) {
        // Get the last message in the array (most recent user input)
        if let Some(last) = arr.last().and_then(|v| v.as_str()) {
            return Some(last.to_string());
        }
    }

    // Generic fallbacks: "message", "text", "user_prompt", "input", "query"
    for field in ["message", "text", "user_prompt", "input", "query"] {
        if let Some(val) = obj.get(field).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
    }

    None
}

/// Extract tool name from various provider formats.
///
/// Parameters:
///   - data: Raw JSON payload
///
/// Returns: Tool name if found
fn extract_tool_name(data: &Value) -> Option<String> {
    let obj = data.as_object()?;

    // Claude Code, Cursor, Gemini: "tool_name" (snake_case)
    if let Some(val) = obj.get("tool_name").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    // OpenClaw: "toolName" (camelCase)
    if let Some(val) = obj.get("toolName").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    None
}

/// Extract tool input/parameters from various provider formats.
///
/// Parameters:
///   - data: Raw JSON payload
///
/// Returns: Tool input as JSON Value if found
fn extract_tool_input(data: &Value) -> Option<Value> {
    let obj = data.as_object()?;

    // Claude Code, Cursor, Gemini: "tool_input"
    if let Some(val) = obj.get("tool_input") {
        return Some(val.clone());
    }

    // OpenClaw: "params"
    if let Some(val) = obj.get("params") {
        return Some(val.clone());
    }

    None
}

/// Extract session/thread identifier from various provider formats.
///
/// Parameters:
///   - data: Raw JSON payload
///
/// Returns: Session ID if found
fn extract_session_id(data: &Value) -> Option<String> {
    let obj = data.as_object()?;

    // Claude Code, Cursor: "session_id"
    if let Some(val) = obj.get("session_id").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    // Codex: "thread-id"
    if let Some(val) = obj.get("thread-id").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    // OpenClaw: "sessionKey"
    if let Some(val) = obj.get("sessionKey").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    // OpenClaw alternative: "channelId"
    if let Some(val) = obj.get("channelId").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }

    None
}

/// Extract working directory from payload.
///
/// Parameters:
///   - data: Raw JSON payload
///
/// Returns: Working directory path if found
fn extract_cwd(data: &Value) -> Option<String> {
    data.as_object()?
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(String::from)
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudQueryType {
    Auth,
    Notify,
}

#[derive(Display)]
pub enum CloudVerdict {
    Allow,
    Deny(String),
}

#[derive(Deserialize)]
struct CloudResponse {
    success: bool,
    reason: Option<String>,
    #[allow(dead_code)]
    error: Option<String>,
    #[allow(dead_code)]
    rejected: Option<bool>,
    rule: Option<String>,
}

impl CloudResponse {
    pub fn block_message(&self) -> String {
        let mut parts = Vec::new();

        parts.push("Command blocked by policy.".to_string());

        if let Some(reason) = &self.reason {
            parts.push(format!("Reason: {reason}"));
        }

        if let Some(rule) = &self.rule {
            parts.push(format!("Rule: {rule}"));
        }

        if let Some(error) = &self.error {
            parts.push(format!("Error: {error}"));
        }

        parts.join(" ")
    }
}

#[derive(Serialize)]
struct CloudRequestMetaVersion {
    version: &'static str,
    hash: &'static str,
}

#[derive(Serialize)]
struct CloudRequestMeta<'a> {
    ts: u128,
    installation_id: &'a str,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    source: &'a Providers,
    #[serde(rename = "type")]
    query_type: CloudQueryType,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    version: CloudRequestMetaVersion,
}

#[derive(Serialize)]
struct CloudRequest<'a> {
    meta_data: CloudRequestMeta<'a>,
    /// Normalized event fields for consistent D&R rule matching across providers
    normalized: NormalizedEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notify: Option<Value>,
}

pub struct CloudQuery<'a> {
    config: &'a Config,
    url: String,
    secret: String,
    provider: Providers,
}

/// Mine session ID from payload using the normalized extraction.
/// Logs a warning if no session ID is found.
///
/// Parameters:
///   - data: Raw JSON payload
///
/// Returns: Session ID if found
fn mine_session_id(data: &Value) -> Option<String> {
    let session_id = extract_session_id(data);

    if session_id.is_none() {
        warn!("Unable to find a session id in hook data");
    }

    session_id
}

impl<'a> CloudRequestMeta<'a> {
    pub fn new(
        config: &'a Config,
        session_id: Option<String>,
        source: &'a Providers,
        query_type: CloudQueryType,
    ) -> Result<Self> {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Unable to get current timestamp")?
            .as_millis();

        let installation_id = config.install_id.as_str();
        let request_id = Uuid::new_v4().to_string();

        let hostname = if let Ok(host) = hostname::get() {
            if let Ok(host) = host.into_string() {
                Some(host)
            } else {
                warn!("Unable to get localhostname");
                None
            }
        } else {
            warn!("Unable to get localhostname");
            None
        };

        let version = CloudRequestMetaVersion {
            version: PROJECT_VERSION,
            hash: PROJECT_VERSION_HASH,
        };

        let username = whoami::username().ok();

        Ok(Self {
            ts,
            installation_id,
            request_id,
            hostname,
            session_id,
            source,
            query_type,
            username,
            version,
        })
    }
}

impl<'a> CloudQuery<'a> {
    pub fn new(config: &'a Config, provider: Providers) -> Result<Self> {
        //
        // bail if we're not actually yet authorized
        //
        if !config.org.authorized() {
            display_authorize_help();
            bail!("Not yet authorized")
        }

        info!("Authorized for oid={}", config.org.oid);

        // Parse the URL and extract the secret from the last path segment
        // URL format: https://{hooks_domain}/{oid}/{adapter_name}/{secret}
        let (url, secret) = Self::extract_secret_from_url(&config.org.url)
            .with_context(|| format!("Unable to get secret from {}", config.org.url))?;

        info!("Using url={url}");

        Ok(Self {
            config,
            url,
            secret,
            provider,
        })
    }

    /// Extract the secret from the webhook URL and return the URL without it.
    /// The secret is sent via header to avoid proxies logging it in access logs.
    fn extract_secret_from_url(full_url: &str) -> Result<(String, String)> {
        let mut parsed = url::Url::parse(full_url).context("Invalid webhook URL format")?;

        // Get path segments and extract the last one as the secret
        let segments: Vec<&str> = parsed
            .path_segments()
            .context("Webhook URL has no path segments")?
            .collect();

        if segments.len() < 3 {
            bail!("Invalid webhook URL format. Expected: https://hooks.domain/oid/name/secret");
        }

        // The last segment is the secret
        let secret = segments
            .last()
            .context("No secret segment in URL")?
            .to_string();

        if secret.is_empty() {
            bail!("Secret segment in webhook URL cannot be empty");
        }

        // Rebuild the path without the secret (we know segments.len() >= 3)
        let path_without_secret: String = segments
            .get(..segments.len().saturating_sub(1))
            .unwrap_or(&[])
            .join("/");
        parsed.set_path(&format!("/{path_without_secret}"));

        Ok((parsed.to_string(), secret))
    }

    /// Send a notification to the cloud (for prompt/audit events).
    /// Notifications are fire-and-forget and don't affect the hook decision.
    ///
    /// Parameters:
    ///   - data: Raw JSON payload from the provider hook
    ///
    /// Returns: Result indicating success or failure
    pub fn notify(&self, data: Value) -> Result<()> {
        debug!("Preparing notification request to cloud");
        let session_id = mine_session_id(&data);
        debug!("Session ID: {session_id:?}");

        // Create normalized event for consistent D&R rule matching
        // Notifications are prompt events (is_tool_use=false)
        let normalized = NormalizedEvent::from_payload(&data, false);
        if let Ok(pretty) = serde_json::to_string_pretty(&normalized) {
            debug!("NORMALIZED_EVENT:\n{pretty}");
        }

        let meta_data = CloudRequestMeta::new(
            self.config,
            session_id,
            &self.provider,
            CloudQueryType::Notify,
        )?;
        let req = CloudRequest {
            meta_data,
            normalized,
            notify: Some(data),
            auth: None,
        };

        // Log the full request being sent to LimaCharlie
        if let Ok(pretty) = serde_json::to_string_pretty(&req) {
            debug!("CLOUD_REQUEST (notify):\n{pretty}");
        }

        debug!("Sending notification to: {}", self.url);
        let ret = minreq::post(&self.url)
            .with_timeout(CLOUD_API_TIMEOUT_SECS)
            .with_header("User-Agent", user_agent())
            .with_header("lc-secret", &self.secret)
            .with_json(&req)
            .context("Failed to serialize notification request")?
            .send();

        match &ret {
            Ok(response) => {
                debug!("Notification response: status={}", response.status_code);
                info!("Notification sent successfully");
            }
            Err(e) => {
                error!("Notification to {} failed: {e}", self.url);
            }
        }

        Ok(())
    }

    /// Send an authorization request to the cloud (for `tool_use` events).
    /// Returns a verdict that determines whether the tool call should proceed.
    ///
    /// Parameters:
    ///   - data: Raw JSON payload from the provider hook
    ///
    /// Returns: `CloudVerdict` (Allow or Deny with reason)
    pub fn authorize(&self, data: Value) -> Result<CloudVerdict> {
        debug!("Preparing authorization request to cloud");
        let session_id = mine_session_id(&data);
        debug!("Session ID: {session_id:?}");

        // Create normalized event for consistent D&R rule matching
        // Authorizations are tool_use events (is_tool_use=true)
        let normalized = NormalizedEvent::from_payload(&data, true);
        if let Ok(pretty) = serde_json::to_string_pretty(&normalized) {
            debug!("NORMALIZED_EVENT:\n{pretty}");
        }

        let meta_data = CloudRequestMeta::new(
            self.config,
            session_id,
            &self.provider,
            CloudQueryType::Auth,
        )?;

        let req = CloudRequest {
            meta_data,
            normalized,
            auth: Some(data),
            notify: None,
        };

        // Log the full request being sent to LimaCharlie
        if let Ok(pretty) = serde_json::to_string_pretty(&req) {
            debug!("CLOUD_REQUEST (auth):\n{pretty}");
        }

        debug!("Sending authorization to: {}", self.url);
        debug!("Timeout: {CLOUD_API_TIMEOUT_SECS}s");

        let res = minreq::post(&self.url)
            .with_timeout(CLOUD_API_TIMEOUT_SECS)
            .with_header("User-Agent", user_agent())
            .with_header("lc-secret", &self.secret)
            .with_json(&req)
            .context("Failed to serialize authorization request")?
            .send()
            .with_context(|| format!("Failed to connect to hook server at {}", self.url))?;

        debug!("Authorization response: status={}", res.status_code);

        if !(200..300).contains(&res.status_code) {
            let error_body = res.as_str().unwrap_or("Unknown error");
            anyhow::bail!(
                "Authorization request failed with status {}: {}",
                res.status_code,
                error_body
            );
        }

        let data = res.as_str()?;
        debug!("Cloud response body: {data}");

        let data: CloudResponse = res
            .json()
            .context("Authorization server returned invalid JSON response")?;

        info!("Authorization result: allow={} reason={:?}", data.success, data.reason);

        let verdict = if data.success {
            CloudVerdict::Allow
        } else {
            let msg = data.block_message();
            debug!("Block message: {msg}");
            CloudVerdict::Deny(msg)
        };

        Ok(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // NormalizedEvent extraction tests - Claude Code format
    // =========================================================================

    #[test]
    fn test_normalize_claude_code_prompt() {
        let payload = json!({
            "prompt": "help me write a function",
            "session_id": "session-abc-123",
            "cwd": "/home/user/project",
            "hook_event_name": "UserPromptSubmit"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert_eq!(normalized.prompt_text, Some("help me write a function".to_string()));
        assert_eq!(normalized.session_id, Some("session-abc-123".to_string()));
        assert_eq!(normalized.cwd, Some("/home/user/project".to_string()));
        assert!(normalized.tool_name.is_none());
        assert!(normalized.tool_input.is_none());
    }

    #[test]
    fn test_normalize_claude_code_tool_use() {
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {
                "command": "cargo build"
            },
            "tool_use_id": "toolu_01XYZ789",
            "session_id": "session-abc-123",
            "cwd": "/home/user/project"
        });

        let normalized = NormalizedEvent::from_payload(&payload, true);

        assert_eq!(normalized.event_type, "tool_use");
        assert_eq!(normalized.tool_name, Some("Bash".to_string()));
        assert_eq!(normalized.tool_input, Some(json!({"command": "cargo build"})));
        assert_eq!(normalized.session_id, Some("session-abc-123".to_string()));
        assert_eq!(normalized.cwd, Some("/home/user/project".to_string()));
    }

    // =========================================================================
    // NormalizedEvent extraction tests - Cursor format
    // =========================================================================

    #[test]
    fn test_normalize_cursor_prompt() {
        let payload = json!({
            "prompt": "Please help me write a function",
            "session_id": "cursor-session-123"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert_eq!(normalized.prompt_text, Some("Please help me write a function".to_string()));
        assert_eq!(normalized.session_id, Some("cursor-session-123".to_string()));
    }

    #[test]
    fn test_normalize_cursor_tool_use() {
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {
                "command": "npm install"
            },
            "tool_use_id": "toolu_cursor_123",
            "session_id": "cursor-session-123"
        });

        let normalized = NormalizedEvent::from_payload(&payload, true);

        assert_eq!(normalized.event_type, "tool_use");
        assert_eq!(normalized.tool_name, Some("Bash".to_string()));
        assert_eq!(normalized.tool_input, Some(json!({"command": "npm install"})));
        assert_eq!(normalized.session_id, Some("cursor-session-123".to_string()));
    }

    // =========================================================================
    // NormalizedEvent extraction tests - OpenClaw format (camelCase!)
    // =========================================================================

    #[test]
    fn test_normalize_openclaw_message_received() {
        let payload = json!({
            "eventType": "message_received",
            "content": "Can you help me debug this?",
            "role": "user",
            "timestamp": "2024-01-15T10:30:00Z",
            "agentId": "agent-123",
            "sessionKey": "openclaw-session-456"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert_eq!(normalized.prompt_text, Some("Can you help me debug this?".to_string()));
        assert_eq!(normalized.session_id, Some("openclaw-session-456".to_string()));
    }

    #[test]
    fn test_normalize_openclaw_before_tool_call() {
        // OpenClaw uses camelCase: toolName, params, sessionKey
        let payload = json!({
            "eventType": "before_tool_call",
            "toolName": "execute_shell",
            "params": {
                "command": "npm install"
            },
            "agentId": "agent-123",
            "sessionKey": "openclaw-session-456"
        });

        let normalized = NormalizedEvent::from_payload(&payload, true);

        assert_eq!(normalized.event_type, "tool_use");
        assert_eq!(normalized.tool_name, Some("execute_shell".to_string()));
        assert_eq!(normalized.tool_input, Some(json!({"command": "npm install"})));
        assert_eq!(normalized.session_id, Some("openclaw-session-456".to_string()));
    }

    #[test]
    fn test_normalize_openclaw_channel_id_fallback() {
        // OpenClaw may use channelId instead of sessionKey
        let payload = json!({
            "eventType": "message_received",
            "content": "Hello",
            "channelId": "channel-xyz"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.session_id, Some("channel-xyz".to_string()));
    }

    // =========================================================================
    // NormalizedEvent extraction tests - Codex format
    // =========================================================================

    #[test]
    fn test_normalize_codex_prompt() {
        // Codex uses input-messages array and thread-id
        let payload = json!({
            "type": "agent-turn-complete",
            "input-messages": [
                "first message",
                "second message",
                "latest user input"
            ],
            "last-assistant-message": "I'll help with that.",
            "thread-id": "codex-thread-123",
            "turn-id": "5",
            "cwd": "/home/user/project"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        // Should extract the LAST message from the array
        assert_eq!(normalized.prompt_text, Some("latest user input".to_string()));
        assert_eq!(normalized.session_id, Some("codex-thread-123".to_string()));
        assert_eq!(normalized.cwd, Some("/home/user/project".to_string()));
    }

    #[test]
    fn test_normalize_codex_single_message() {
        let payload = json!({
            "input-messages": ["only one message"],
            "thread-id": "codex-thread-123"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.prompt_text, Some("only one message".to_string()));
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn test_normalize_empty_payload() {
        let payload = json!({});

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert!(normalized.prompt_text.is_none());
        assert!(normalized.tool_name.is_none());
        assert!(normalized.tool_input.is_none());
        assert!(normalized.session_id.is_none());
        assert!(normalized.cwd.is_none());
    }

    #[test]
    fn test_normalize_non_object_payload() {
        let payload = json!("just a string");

        let normalized = NormalizedEvent::from_payload(&payload, true);

        assert_eq!(normalized.event_type, "tool_use");
        assert!(normalized.prompt_text.is_none());
    }

    #[test]
    fn test_normalize_empty_input_messages_array() {
        let payload = json!({
            "input-messages": []
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert!(normalized.prompt_text.is_none());
    }

    // =========================================================================
    // Session ID extraction priority tests
    // =========================================================================

    #[test]
    fn test_session_id_prefers_session_id_over_thread_id() {
        // If both exist, session_id should win
        let payload = json!({
            "session_id": "preferred-session",
            "thread-id": "fallback-thread"
        });

        let session = extract_session_id(&payload);
        assert_eq!(session, Some("preferred-session".to_string()));
    }

    #[test]
    fn test_session_id_falls_back_to_thread_id() {
        let payload = json!({
            "thread-id": "codex-thread"
        });

        let session = extract_session_id(&payload);
        assert_eq!(session, Some("codex-thread".to_string()));
    }

    #[test]
    fn test_session_id_falls_back_to_session_key() {
        let payload = json!({
            "sessionKey": "openclaw-session"
        });

        let session = extract_session_id(&payload);
        assert_eq!(session, Some("openclaw-session".to_string()));
    }

    // =========================================================================
    // Serialization tests
    // =========================================================================

    #[test]
    fn test_normalized_event_serialization() {
        let normalized = NormalizedEvent {
            event_type: "tool_use".to_string(),
            prompt_text: None,
            tool_name: Some("Bash".to_string()),
            tool_input: Some(json!({"command": "ls"})),
            session_id: Some("session-123".to_string()),
            cwd: Some("/home/user".to_string()),
        };

        let json = serde_json::to_string(&normalized).unwrap();

        // Should NOT include null fields (skip_serializing_if)
        assert!(!json.contains("prompt_text"));
        assert!(json.contains("\"event_type\":\"tool_use\""));
        assert!(json.contains("\"tool_name\":\"Bash\""));
    }

    #[test]
    fn test_normalized_event_skips_none_fields() {
        let normalized = NormalizedEvent {
            event_type: "prompt".to_string(),
            prompt_text: Some("hello".to_string()),
            tool_name: None,
            tool_input: None,
            session_id: None,
            cwd: None,
        };

        let json = serde_json::to_string(&normalized).unwrap();

        // Only event_type and prompt_text should be present
        assert!(json.contains("event_type"));
        assert!(json.contains("prompt_text"));
        assert!(!json.contains("tool_name"));
        assert!(!json.contains("tool_input"));
        assert!(!json.contains("session_id"));
        assert!(!json.contains("cwd"));
    }

    // =========================================================================
    // Prompt text extraction - all PROMPT_HINTS fallbacks
    // =========================================================================

    #[test]
    fn test_prompt_text_extracts_message_field() {
        let payload = json!({"message": "user message here"});
        assert_eq!(extract_prompt_text(&payload), Some("user message here".to_string()));
    }

    #[test]
    fn test_prompt_text_extracts_text_field() {
        let payload = json!({"text": "text content here"});
        assert_eq!(extract_prompt_text(&payload), Some("text content here".to_string()));
    }

    #[test]
    fn test_prompt_text_extracts_user_prompt_field() {
        let payload = json!({"user_prompt": "user prompt here"});
        assert_eq!(extract_prompt_text(&payload), Some("user prompt here".to_string()));
    }

    #[test]
    fn test_prompt_text_extracts_input_field() {
        let payload = json!({"input": "input text here"});
        assert_eq!(extract_prompt_text(&payload), Some("input text here".to_string()));
    }

    #[test]
    fn test_prompt_text_extracts_query_field() {
        let payload = json!({"query": "search query here"});
        assert_eq!(extract_prompt_text(&payload), Some("search query here".to_string()));
    }

    #[test]
    fn test_prompt_text_priority_prompt_over_content() {
        // "prompt" should be checked before "content"
        let payload = json!({
            "prompt": "primary prompt",
            "content": "secondary content"
        });
        assert_eq!(extract_prompt_text(&payload), Some("primary prompt".to_string()));
    }

    #[test]
    fn test_prompt_text_priority_content_over_message() {
        // "content" should be checked before "message"
        let payload = json!({
            "content": "primary content",
            "message": "secondary message"
        });
        assert_eq!(extract_prompt_text(&payload), Some("primary content".to_string()));
    }

    // =========================================================================
    // Tool name extraction edge cases
    // =========================================================================

    #[test]
    fn test_tool_name_prefers_snake_case_over_camel_case() {
        // If both exist, tool_name (snake_case) should win
        let payload = json!({
            "tool_name": "Bash",
            "toolName": "execute_shell"
        });
        assert_eq!(extract_tool_name(&payload), Some("Bash".to_string()));
    }

    #[test]
    fn test_tool_name_empty_string() {
        let payload = json!({"tool_name": ""});
        assert_eq!(extract_tool_name(&payload), Some(String::new()));
    }

    #[test]
    fn test_tool_name_non_string_ignored() {
        let payload = json!({"tool_name": 123});
        assert_eq!(extract_tool_name(&payload), None);
    }

    #[test]
    fn test_tool_name_null_ignored() {
        let payload = json!({"tool_name": null});
        assert_eq!(extract_tool_name(&payload), None);
    }

    // =========================================================================
    // Tool input extraction edge cases
    // =========================================================================

    #[test]
    fn test_tool_input_complex_nested_structure() {
        let payload = json!({
            "tool_input": {
                "command": "find . -name '*.rs'",
                "options": {
                    "recursive": true,
                    "exclude": ["target", "node_modules"]
                },
                "env": {
                    "PATH": "/usr/bin"
                }
            }
        });

        let input = extract_tool_input(&payload).unwrap();
        assert_eq!(input["command"], "find . -name '*.rs'");
        assert_eq!(input["options"]["recursive"], true);
        assert_eq!(input["options"]["exclude"][0], "target");
    }

    #[test]
    fn test_tool_input_prefers_tool_input_over_params() {
        let payload = json!({
            "tool_input": {"primary": true},
            "params": {"secondary": true}
        });
        let input = extract_tool_input(&payload).unwrap();
        assert_eq!(input["primary"], true);
        assert!(input.get("secondary").is_none());
    }

    #[test]
    fn test_tool_input_string_value() {
        // tool_input could be a string in some cases
        let payload = json!({"tool_input": "simple command"});
        let input = extract_tool_input(&payload).unwrap();
        assert_eq!(input, "simple command");
    }

    #[test]
    fn test_tool_input_array_value() {
        let payload = json!({"tool_input": ["arg1", "arg2", "arg3"]});
        let input = extract_tool_input(&payload).unwrap();
        assert_eq!(input[0], "arg1");
        assert_eq!(input.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_tool_input_null_returns_none() {
        let payload = json!({"tool_input": null});
        // null is still a valid Value, so it will be returned
        let input = extract_tool_input(&payload);
        assert!(input.is_some());
        assert!(input.unwrap().is_null());
    }

    // =========================================================================
    // Session ID extraction edge cases
    // =========================================================================

    #[test]
    fn test_session_id_non_string_ignored() {
        let payload = json!({"session_id": 12345});
        assert_eq!(extract_session_id(&payload), None);
    }

    #[test]
    fn test_session_id_null_ignored() {
        let payload = json!({"session_id": null});
        assert_eq!(extract_session_id(&payload), None);
    }

    #[test]
    fn test_session_id_empty_string() {
        let payload = json!({"session_id": ""});
        assert_eq!(extract_session_id(&payload), Some(String::new()));
    }

    #[test]
    fn test_session_id_full_priority_chain() {
        // Test the full fallback chain: session_id > thread-id > sessionKey > channelId
        let payload = json!({"channelId": "last-resort"});
        assert_eq!(extract_session_id(&payload), Some("last-resort".to_string()));
    }

    // =========================================================================
    // CWD extraction edge cases
    // =========================================================================

    #[test]
    fn test_cwd_extraction() {
        let payload = json!({"cwd": "/home/user/project"});
        assert_eq!(extract_cwd(&payload), Some("/home/user/project".to_string()));
    }

    #[test]
    fn test_cwd_non_string_ignored() {
        let payload = json!({"cwd": ["not", "a", "string"]});
        assert_eq!(extract_cwd(&payload), None);
    }

    #[test]
    fn test_cwd_null_ignored() {
        let payload = json!({"cwd": null});
        assert_eq!(extract_cwd(&payload), None);
    }

    // =========================================================================
    // Unicode and special characters
    // =========================================================================

    #[test]
    fn test_prompt_text_unicode() {
        let payload = json!({"prompt": "Help me with 日本語 and émojis 🎉"});
        let normalized = NormalizedEvent::from_payload(&payload, false);
        assert_eq!(normalized.prompt_text, Some("Help me with 日本語 and émojis 🎉".to_string()));
    }

    #[test]
    fn test_tool_name_unicode() {
        let payload = json!({"tool_name": "工具名"});
        assert_eq!(extract_tool_name(&payload), Some("工具名".to_string()));
    }

    #[test]
    fn test_prompt_with_newlines_and_special_chars() {
        let payload = json!({"prompt": "Line 1\nLine 2\tTabbed\r\nWindows line"});
        let normalized = NormalizedEvent::from_payload(&payload, false);
        assert_eq!(
            normalized.prompt_text,
            Some("Line 1\nLine 2\tTabbed\r\nWindows line".to_string())
        );
    }

    // =========================================================================
    // Real-world payload examples from debug logs
    // =========================================================================

    #[test]
    fn test_real_claude_code_user_prompt_submit() {
        // Actual payload from debug logs
        let payload = json!({
            "cwd": "/home/tomaz/w/refractionPOINT/viberails",
            "hook_event_name": "UserPromptSubmit",
            "permission_mode": "default",
            "prompt": "run ls -la command",
            "session_id": "084b8954-d210-4fdc-ab4c-d155ff79d469",
            "transcript_path": "/home/tomaz/.claude/projects/-home-tomaz-w-refractionPOINT-viberails/084b8954-d210-4fdc-ab4c-d155ff79d469.jsonl"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert_eq!(normalized.prompt_text, Some("run ls -la command".to_string()));
        assert_eq!(normalized.session_id, Some("084b8954-d210-4fdc-ab4c-d155ff79d469".to_string()));
        assert_eq!(normalized.cwd, Some("/home/tomaz/w/refractionPOINT/viberails".to_string()));
        assert!(normalized.tool_name.is_none());
    }

    #[test]
    fn test_real_claude_code_pre_tool_use() {
        // Actual payload from debug logs
        let payload = json!({
            "cwd": "/home/tomaz/w/refractionPOINT/viberails",
            "hook_event_name": "PreToolUse",
            "permission_mode": "default",
            "session_id": "084b8954-d210-4fdc-ab4c-d155ff79d469",
            "tool_input": {
                "command": "ls -la",
                "description": "List all files with details"
            },
            "tool_name": "Bash",
            "tool_use_id": "toolu_019MiRS1zAkSYdFKtc7Ee4C7",
            "transcript_path": "/home/tomaz/.claude/projects/-home-tomaz-w-refractionPOINT-viberails/084b8954-d210-4fdc-ab4c-d155ff79d469.jsonl"
        });

        let normalized = NormalizedEvent::from_payload(&payload, true);

        assert_eq!(normalized.event_type, "tool_use");
        assert_eq!(normalized.tool_name, Some("Bash".to_string()));
        assert_eq!(normalized.tool_input.as_ref().unwrap()["command"], "ls -la");
        assert_eq!(normalized.session_id, Some("084b8954-d210-4fdc-ab4c-d155ff79d469".to_string()));
        assert_eq!(normalized.cwd, Some("/home/tomaz/w/refractionPOINT/viberails".to_string()));
    }

    #[test]
    fn test_real_codex_agent_turn_complete() {
        // Actual payload from debug logs
        let payload = json!({
            "cwd": "/home/tomaz/w/refractionPOINT/viberails",
            "input-messages": [
                "run ls -la",
                "run uptime command"
            ],
            "last-assistant-message": "18:55:34 up 21 days, 10:18, 0 user, load average: 0.62, 1.11, 0.97",
            "thread-id": "019c24a5-1a9b-74a2-9c81-59a447bc16d7",
            "turn-id": "4",
            "type": "agent-turn-complete"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        // Should get the LAST message
        assert_eq!(normalized.prompt_text, Some("run uptime command".to_string()));
        assert_eq!(normalized.session_id, Some("019c24a5-1a9b-74a2-9c81-59a447bc16d7".to_string()));
        assert_eq!(normalized.cwd, Some("/home/tomaz/w/refractionPOINT/viberails".to_string()));
    }

    // =========================================================================
    // Gemini CLI format tests
    // =========================================================================

    #[test]
    fn test_normalize_gemini_tool_use() {
        // Gemini uses same format as Claude Code
        let payload = json!({
            "tool_name": "Read",
            "tool_input": {
                "file_path": "/etc/passwd"
            },
            "tool_use_id": "gemini_tool_123"
        });

        let normalized = NormalizedEvent::from_payload(&payload, true);

        assert_eq!(normalized.event_type, "tool_use");
        assert_eq!(normalized.tool_name, Some("Read".to_string()));
        assert_eq!(normalized.tool_input.as_ref().unwrap()["file_path"], "/etc/passwd");
    }

    // =========================================================================
    // Codex edge cases
    // =========================================================================

    #[test]
    fn test_codex_input_messages_with_non_string_elements() {
        // If input-messages contains non-strings, should handle gracefully
        let payload = json!({
            "input-messages": [123, "valid string", null, {"obj": true}]
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        // Should not panic, but won't extract the last element (it's an object)
        // Will try to get as_str() on the object which returns None
        // So it should fall through and return None
        assert!(normalized.prompt_text.is_none());
    }

    #[test]
    fn test_codex_input_messages_last_is_string() {
        let payload = json!({
            "input-messages": [123, null, "last valid string"]
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);
        assert_eq!(normalized.prompt_text, Some("last valid string".to_string()));
    }

    // =========================================================================
    // Payload type tests
    // =========================================================================

    #[test]
    fn test_array_payload() {
        let payload = json!(["not", "an", "object"]);
        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert!(normalized.prompt_text.is_none());
        assert!(normalized.tool_name.is_none());
    }

    #[test]
    fn test_number_payload() {
        let payload = json!(42);
        let normalized = NormalizedEvent::from_payload(&payload, true);

        assert_eq!(normalized.event_type, "tool_use");
        assert!(normalized.tool_name.is_none());
    }

    #[test]
    fn test_boolean_payload() {
        let payload = json!(true);
        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert!(normalized.prompt_text.is_none());
    }

    #[test]
    fn test_null_payload() {
        let payload = json!(null);
        let normalized = NormalizedEvent::from_payload(&payload, false);

        assert_eq!(normalized.event_type, "prompt");
        assert!(normalized.prompt_text.is_none());
    }

    // =========================================================================
    // Integration-style tests
    // =========================================================================

    #[test]
    fn test_full_normalized_event_for_dr_rules() {
        // Simulate what a D&R rule would see
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": {
                "command": "rm -rf /important/data",
                "description": "Delete files"
            },
            "session_id": "dangerous-session",
            "cwd": "/root"
        });

        let normalized = NormalizedEvent::from_payload(&payload, true);

        // D&R rule could check:
        // event.normalized.tool_name == "Bash"
        assert_eq!(normalized.tool_name.as_deref(), Some("Bash"));

        // event.normalized.tool_input.command CONTAINS "rm -rf"
        let cmd = normalized.tool_input.as_ref().unwrap()["command"].as_str().unwrap();
        assert!(cmd.contains("rm -rf"));

        // event.normalized.cwd == "/root"
        assert_eq!(normalized.cwd.as_deref(), Some("/root"));
    }

    #[test]
    fn test_full_normalized_prompt_for_dr_rules() {
        let payload = json!({
            "prompt": "Please delete all production data immediately",
            "session_id": "suspicious-session",
            "cwd": "/var/www/production"
        });

        let normalized = NormalizedEvent::from_payload(&payload, false);

        // D&R rule could check:
        // event.normalized.prompt_text CONTAINS "delete"
        let prompt = normalized.prompt_text.as_ref().unwrap();
        assert!(prompt.contains("delete"));

        // event.normalized.prompt_text CONTAINS "production"
        assert!(prompt.contains("production"));
    }
}
