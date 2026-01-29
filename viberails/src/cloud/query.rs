use std::time::SystemTime;

use anyhow::{Context, Result};
use derive_more::Display;
use serde::{Deserialize, Serialize};

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
struct CloudRequest<'a> {
    ts: u128,
    hook_data: &'a str,
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

    pub fn authorize<S>(&self, data: S) -> Result<CloudVerdict>
    where
        S: AsRef<str>,
    {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_millis();

        let req = CloudRequest {
            ts,
            hook_data: data.as_ref(),
        };

        let res = minreq::post(&self.config.authorize_url)
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
