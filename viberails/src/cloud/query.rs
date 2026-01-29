use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use derive_more::Display;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
struct CloudRequest {
    ts: u128,
    data: Value,
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

impl CloudRequest {
    pub fn new(data: Value) -> Result<Self> {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Unable to get current timestamp")?
            .as_millis();

        let session_id = find_session_id(&data);

        Ok(Self {
            ts,
            data,
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
        let req = CloudRequest::new(data)?;

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
