use std::time::SystemTime;

use anyhow::Result;
use derive_more::Display;
use serde::{Deserialize, Serialize};

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
    server_url: String,
}

impl CloudQuery {
    pub fn new<U>(server_url: U) -> Result<Self>
    where
        U: Into<String>,
    {
        Ok(Self {
            server_url: server_url.into(),
        })
    }

    pub fn query<S>(&self, data: S) -> Result<CloudVerdict>
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

        let res = minreq::post(&self.server_url).with_json(&req)?.send()?;

        let data: CloudResponse = res.json()?;

        let verdict = if data.allow {
            CloudVerdict::Allow
        } else {
            CloudVerdict::Deny(data.reason)
        };

        Ok(verdict)
    }
}
