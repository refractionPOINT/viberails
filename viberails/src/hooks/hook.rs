use std::{
    io::{BufRead, BufReader, BufWriter, Stdout, Write, stdin, stdout},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use derive_more::Display;
use log::{error, info, warn};
use serde::Serialize;

use crate::{
    cloud::{CloudQuery, CloudVerdict},
    common::PROJECT_NAME,
    logging::init_logging,
};

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

fn write_decision(out: &mut BufWriter<Stdout>, decision: HookDecision) -> Result<()> {
    let answer: HookAnswer = decision.into();

    let resp_string = serde_json::to_string(&answer)?;

    out.write_all(resp_string.as_bytes())?;
    out.flush()?;

    Ok(())
}

fn io_loop() -> Result<()> {
    let cloud = CloudQuery::new("ok")?;

    let stdin = stdin();
    let stdout = stdout();

    let mut rdr = BufReader::new(stdin);
    let mut writer = BufWriter::new(stdout);

    let mut line = String::new();

    loop {
        line.clear();

        let len = match rdr.read_line(&mut line) {
            Ok(v) => v,
            Err(e) => {
                bail!("Unable to read from stdin ({e})");
            }
        };

        if 0 == len {
            // that's still successful, out input just got closed
            warn!("EOL we're leaving");
            break; // EOF
        }

        //
        // Query D&R
        //
        let start = Instant::now();

        //
        // Do we fail-open?
        //
        let decision = match cloud.query(&line) {
            Ok(CloudVerdict::Allow) => HookDecision::Ignore,
            Ok(CloudVerdict::Deny(r)) => {
                warn!("Deny reason: {r}");
                HookDecision::Block(r)
            }
            Err(e) => {
                error!("cloud failed ({e})");
                //
                // Fail-open. This should be a configuration
                //
                HookDecision::Ignore
            }
        };

        let duration = start.elapsed().as_millis();
        info!("Desision={decision} duration={duration}ms");

        write_decision(&mut writer, decision)?;
    }

    Ok(())
}

pub fn hook() -> Result<()> {
    let pid = std::process::id();
    let file_name = format!("{PROJECT_NAME}_{pid}.log");

    init_logging(Some(file_name)).context("Unable to initialized logging")?;

    info!("{PROJECT_NAME} is starting");

    io_loop()
}
