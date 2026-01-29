use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use derive_more::Display;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::config::Config;

#[derive(Display)]
pub enum CloudVerdict {
    Allow,
    Deny(String),
}

#[derive(Deserialize)]
struct CloudResponse {
    allow: bool,
    reason: String,
}

#[derive(Serialize)]
struct CloudRequestMeta<'a> {
    ts: u128,
    installation_id: &'a str,
    request_id: String,
}

#[derive(Serialize)]
struct CloudRequest<'a> {
    meta_data: CloudRequestMeta<'a>,
    hook_data: Value,
    session_id: Option<String>,
}

fn find_session_id(data: &Value) -> Option<String> {
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
    warn!("Unable to find a session id in {data}");
    None
}

impl<'a> CloudRequestMeta<'a> {
    pub fn new(config: &'a Config) -> Result<Self> {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Unable to get current timestamp")?
            .as_millis();

        let installation_id = config.install_id.as_str();
        let request_id = Uuid::new_v4().to_string();

        Ok(Self {
            ts,
            installation_id,
            request_id,
        })
    }
}

impl<'a> CloudRequest<'a> {
    pub fn new(config: &'a Config, hook_data: Value) -> Result<Self> {
        let session_id = find_session_id(&hook_data);

        let meta_data = CloudRequestMeta::new(config)?;

        Ok(Self {
            meta_data,
            hook_data,
            session_id,
        })
    }
}

pub struct CloudQuery {
    config: Config,
}

impl CloudQuery {
    pub fn new() -> Result<Self> {
        let config = Config::load().context("Unable to load config file")?;

        Ok(Self { config })
    }

    pub fn _notify<S>(&self, _data: S) -> Result<()>
    where
        S: AsRef<str>,
    {
        Ok(())
    }

    pub fn _authenticate(&self) -> Result<()> {
        //
        // Look if we already have a usable token
        //

        bail!("Not Implemented");
    }

    pub fn authorize(&self, data: Value) -> Result<CloudVerdict> {
        let req = CloudRequest::new(&self.config, data)?;

        let res = minreq::post(&self.config.user.authorize_url)
            .with_json(&req)?
            .send()?;

        let data: CloudResponse = res.json()?;

        let verdict = if data.allow {
            CloudVerdict::Allow
        } else {
            CloudVerdict::Deny(data.reason)
        };

        Ok(verdict)
    }
}
