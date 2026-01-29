use std::{
    io::{BufRead, BufReader, BufWriter, Stdin, Stdout, Write, stdin, stdout},
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

fn is_tool_use(value: &Value) -> bool {
    for hint in TOOL_HINTS {
        if value.get(hint).is_some() {
            return true;
        }
    }

    false
}

struct Hook<'a> {
    config: &'a Config,
    cloud: CloudQuery<'a>,
    reader: BufReader<Stdin>,
    writer: BufWriter<Stdout>,
}

impl<'a> Hook<'a> {
    pub fn new(config: &'a Config) -> Self {
        let cloud = CloudQuery::new(config);

        let stdin = stdin();
        let stdout = stdout();

        let reader = BufReader::new(stdin);
        let writer = BufWriter::new(stdout);

        Self {
            config,
            cloud,
            reader,
            writer,
        }
    }

    fn authorize_tool(&self, value: Value) -> HookDecision {
        //
        // Do we fail-open?
        //
        match self.cloud.authorize(value) {
            Ok(CloudVerdict::Allow) => HookDecision::Ignore,
            Ok(CloudVerdict::Deny(r)) => {
                warn!("Deny reason: {r}");
                HookDecision::Block(r)
            }
            Err(e) => {
                error!("cloud failed ({e})");

                if self.config.user.fail_open {
                    HookDecision::Ignore
                } else {
                    let msg = format!("{PROJECT_NAME} cloud failure ({e})");
                    HookDecision::Block(msg)
                }
            }
        }
    }

    fn write_decision(&mut self, decision: HookDecision) -> Result<()> {
        let answer: HookAnswer = decision.into();

        let resp_string =
            serde_json::to_string(&answer).context("Failed to serialize hook response")?;

        self.writer
            .write_all(resp_string.as_bytes())
            .context("Failed to write hook response to stdout")?;
        self.writer
            .flush()
            .context("Failed to flush hook response")?;

        Ok(())
    }

    fn accept(&mut self) -> Result<()> {
        self.write_decision(HookDecision::Ignore)
    }

    fn deny(&mut self) -> Result<()> {
        self.write_decision(HookDecision::Block("Internal Failure".to_string()))
    }

    fn failure_callback(&mut self) -> Result<()> {
        if self.config.user.fail_open {
            self.accept()
        } else {
            self.deny()
        }
    }

    pub fn io_loop(&mut self) -> Result<()> {
        let mut line = String::new();

        loop {
            line.clear();

            // that's a fatal error
            let len = self
                .reader
                .read_line(&mut line)
                .context("Unable to read from stdin")?;

            if 0 == len {
                // that's still successful, out input just got closed
                warn!("EOF. We're leaving");
                break; // EOF
            }

            let Ok(value) = serde_json::from_str(&line) else {
                error!("Unable to parse {line}");
                self.failure_callback()?;
                continue;
            };

            let start = Instant::now();

            let decision = if is_tool_use(&value) {
                //
                // D&R Path
                //
                self.authorize_tool(value)
            } else {
                //
                // This is best effort
                //
                if let Err(e) = self.cloud.notify(value) {
                    error!("Unable to notify cloud ({e})");
                }
                //
                // Notification path ( fire and forget )
                //
                HookDecision::Ignore
            };

            let duration = start.elapsed().as_millis();

            info!("Desision={decision} duration={duration}ms");

            self.write_decision(decision)?;
        }

        Ok(())
    }
}

pub fn hook() -> Result<()> {
    info!("{PROJECT_NAME} is starting");

    let config = Config::load()?;
    let mut hook = Hook::new(&config);

    hook.io_loop()
}
