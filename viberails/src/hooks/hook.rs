use std::{
    io::{BufRead, BufReader, BufWriter, Stdout, Write, stdin, stdout},
    time::Instant,
};

use anyhow::{Context, Result};
use derive_more::Display;
use log::{error, info, warn};
use serde::Serialize;
use serde_json::Value;

use crate::{
    cloud::query::{CloudQuery, CloudVerdict},
    common::PROJECT_NAME,
    config::Config,
};

const TOOL_HINTS: &[&str] = &["tool_input", "tool_name", "tool_use_id"];

#[derive(Serialize, Display)]
#[allow(dead_code)]
enum HookDecision {
    Block(String),
    Allow,  // explicitly permit (skips further hooks)
    Ignore, // no opinion (continue to next hook/permissions)
}

#[derive(Serialize)]
struct HookAnswer {
    decision: HookDecision,
    reason: Option<String>,
}

impl From<HookDecision> for HookAnswer {
    fn from(value: HookDecision) -> Self {
        match value {
            HookDecision::Block(ref r) => {
                let reason = r.clone();
                Self {
                    decision: value,
                    reason: Some(reason),
                }
            }
            _ => Self {
                decision: value,
                reason: None,
            },
        }
    }
}

fn write_decision(writer: &mut BufWriter<Stdout>, decision: HookDecision) -> Result<()> {
    let answer: HookAnswer = decision.into();

    let resp_string = serde_json::to_string(&answer)?;

    writer.write_all(resp_string.as_bytes())?;
    writer.flush()?;

    Ok(())
}

fn is_tool_use(value: &Value) -> bool {
    for hint in TOOL_HINTS {
        if value.get(hint).is_some() {
            return true;
        }
    }

    false
}

fn accept(writer: &mut BufWriter<Stdout>) -> Result<()> {
    write_decision(writer, HookDecision::Ignore)
}

fn deny(writer: &mut BufWriter<Stdout>) -> Result<()> {
    write_decision(writer, HookDecision::Block("Internal Failure".to_string()))
}

fn failure_callback(writer: &mut BufWriter<Stdout>, config: &Config) -> Result<()> {
    if config.user.fail_open {
        accept(writer)
    } else {
        deny(writer)
    }
}

fn authorize_tool(config: &Config, cloud: &CloudQuery, value: Value) -> HookDecision {
    //
    // Do we fail-open?
    //
    match cloud.authorize(value) {
        Ok(CloudVerdict::Allow) => HookDecision::Ignore,
        Ok(CloudVerdict::Deny(r)) => {
            warn!("Deny reason: {r}");
            HookDecision::Block(r)
        }
        Err(e) => {
            error!("cloud failed ({e})");

            if config.user.fail_open {
                HookDecision::Ignore
            } else {
                let msg = format!("{PROJECT_NAME} cloud failure ({e})");
                HookDecision::Block(msg)
            }
        }
    }
}

fn io_loop() -> Result<()> {
    let config = Config::load()?;

    let cloud = CloudQuery::new()?;

    let stdin = stdin();
    let stdout = stdout();

    let mut rdr = BufReader::new(stdin);
    let mut writer = BufWriter::new(stdout);

    let mut line = String::new();

    loop {
        line.clear();

        // that's a fatal error
        let len = rdr
            .read_line(&mut line)
            .context("Unable to read from stdin")?;

        if 0 == len {
            // that's still successful, out input just got closed
            warn!("EOF. We're leaving");
            break; // EOF
        }

        let Ok(value) = serde_json::from_str(&line) else {
            error!("Unable to parse {line}");
            failure_callback(&mut writer, &config)?;
            continue;
        };

        let start = Instant::now();

        let decision = if is_tool_use(&value) {
            //
            // D&R Path
            //
            authorize_tool(&config, &cloud, value)
        } else {
            //
            // This is best effort
            //
            if let Err(e) = cloud.notify(value) {
                error!("Unable to notify cloud ({e})");
            }
            //
            // Notification path ( fire and forget )
            //
            HookDecision::Ignore
        };

        let duration = start.elapsed().as_millis();

        info!("Desision={decision} duration={duration}ms");
    }

    Ok(())
}

pub fn hook() -> Result<()> {
    info!("{PROJECT_NAME} is starting");
    io_loop()
}
