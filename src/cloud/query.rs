use std::time::{Instant, SystemTime};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    ppid: Option<u32>,
    version: CloudRequestMetaVersion,
}

#[derive(Serialize)]
struct CloudRequest<'a> {
    meta_data: CloudRequestMeta<'a>,
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

fn get_ppid() -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, System, UpdateKind};

    let pid = sysinfo::get_current_pid().ok()?;
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Never),
    );
    sys.process(pid)?.parent().map(sysinfo::Pid::as_u32)
}

fn mine_session_id(data: &Value) -> Option<String> {
    //
    // This is to be accomodating for various providers and or versions
    // so we're mining for some kind of session id
    //
    if let Some(session_value) = data.get("session_id")
        && let Some(session_id) = session_value.as_str()
    {
        return Some(session_id.to_string());
    }

    //
    // We'll log it and hopefully it'll percolate so we can fix this
    //
    warn!("Unable to find a session id in hook data");
    None
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
        let ppid = get_ppid();

        Ok(Self {
            ts,
            installation_id,
            request_id,
            hostname,
            session_id,
            source,
            query_type,
            username,
            ppid,
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

    pub fn notify(&self, data: Value) -> Result<()> {
        debug!("Preparing notification request to cloud");
        let session_id = mine_session_id(&data);
        debug!("Session ID: {session_id:?}");

        let meta_data = CloudRequestMeta::new(
            self.config,
            session_id,
            &self.provider,
            CloudQueryType::Notify,
        )?;
        let req = CloudRequest {
            meta_data,
            notify: Some(data),
            auth: None,
        };

        // Log the full request being sent to LimaCharlie
        if let Ok(pretty) = serde_json::to_string_pretty(&req) {
            debug!("CLOUD_REQUEST (notify):\n{pretty}");
        }

        debug!("Sending notification to: {}", self.url);

        // Measure API round-trip latency
        let start = Instant::now();
        let ret = minreq::post(&self.url)
            .with_timeout(CLOUD_API_TIMEOUT_SECS)
            .with_header("User-Agent", user_agent())
            .with_header("lc-secret", &self.secret)
            .with_json(&req)
            .context("Failed to serialize notification request")?
            .send();
        let latency_ms = start.elapsed().as_millis();

        match &ret {
            Ok(response) => {
                debug!("Notification response: status={}", response.status_code);
                info!(
                    "Notification sent (status={}, rtt={}ms)",
                    response.status_code, latency_ms
                );
            }
            Err(e) => {
                error!("Notification failed (rtt={latency_ms}ms): {e}");
            }
        }

        Ok(())
    }

    pub fn authorize(&self, data: Value) -> Result<CloudVerdict> {
        debug!("Preparing authorization request to cloud");
        let session_id = mine_session_id(&data);
        debug!("Session ID: {session_id:?}");

        let meta_data = CloudRequestMeta::new(
            self.config,
            session_id,
            &self.provider,
            CloudQueryType::Auth,
        )?;

        let req = CloudRequest {
            meta_data,
            auth: Some(data),
            notify: None,
        };

        // Log the full request being sent to LimaCharlie
        if let Ok(pretty) = serde_json::to_string_pretty(&req) {
            debug!("CLOUD_REQUEST (auth):\n{pretty}");
        }

        debug!("Sending authorization to: {}", self.url);
        debug!("Timeout: {CLOUD_API_TIMEOUT_SECS}s");

        // Measure API round-trip latency
        let start = Instant::now();
        let res = minreq::post(&self.url)
            .with_timeout(CLOUD_API_TIMEOUT_SECS)
            .with_header("User-Agent", user_agent())
            .with_header("lc-secret", &self.secret)
            .with_json(&req)
            .context("Failed to serialize authorization request")?
            .send()
            .with_context(|| format!("Failed to connect to hook server at {}", self.url))?;
        let latency_ms = start.elapsed().as_millis();

        debug!(
            "Authorization response: status={}, rtt={}ms",
            res.status_code, latency_ms
        );

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

        info!(
            "Authorization result: allow={} reason={:?} (rtt={}ms)",
            data.success, data.reason, latency_ms
        );

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

    #[test]
    fn test_get_ppid_returns_some() {
        let ppid = get_ppid();
        assert!(ppid.is_some(), "get_ppid() should return Some on Unix/Windows");
        assert!(ppid.is_some_and(|p| p > 0), "ppid should be > 0");
    }
}
