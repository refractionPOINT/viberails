use std::{fs, io::Write};

use anyhow::{Context, Result};
use bon::Builder;
use serde::{Deserialize, Serialize};
use tabled::{
    Table, Tabled,
    settings::{Margin, Rotate, Style},
};
use url::Url;

use crate::common::{print_header, project_config_dir};

const DEF_AUTHENTICATION_URL: &str = "http://localhost:8000/auth";
const DEF_AUTHORIZATION_URL: &str = "http://localhost:8000/dnr";
const DEF_NOTIFICATION_URL: &str = "http://localhost:8000/notify";

#[derive(clap::Args)]
pub struct ConfigureArgs {
    /// Authentication URL
    #[arg(long, default_value = DEF_AUTHENTICATION_URL)]
    auth_url: Url,

    /// Authorization URL
    #[arg(long, default_value = DEF_AUTHORIZATION_URL)]
    authorize_url: Url,

    /// Notification URL
    #[arg(long, default_value = DEF_NOTIFICATION_URL)]
    notification_url: Url,

    /// Accept command on cloud failure
    #[arg(long, default_value_t = true)]
    fail_open: bool,
}

#[derive(Serialize, Deserialize, Builder, Tabled)]
pub struct Config {
    pub auth_url: String,
    pub authorize_url: String,
    pub notification_url: String,
    pub fail_open: bool,
}

impl Config {
    pub fn save(&self) -> Result<()> {
        let config_string =
            serde_json::to_string_pretty(self).context("Unable to serialize configuration data")?;

        let config_dir = project_config_dir()?;
        let config_file = config_dir.join("config.json");

        let mut fd = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&config_file)
            .with_context(|| format!("Unable to write {}", config_file.display()))?;

        fd.write_all(config_string.as_bytes())?;

        Ok(())
    }

    pub fn load() -> Result<Self> {
        let config_dir = project_config_dir()?;
        let config_file = config_dir.join("config.json");

        let config_string = fs::read_to_string(&config_file)
            .with_context(|| format!("Unable to read {}", config_file.display()))?;

        let config: Config = serde_json::from_str(&config_string)
            .context("Unable to deserialize configuration data")?;

        Ok(config)
    }
}

pub fn configure(args: &ConfigureArgs) -> Result<()> {
    let config = Config::builder()
        .auth_url(args.auth_url.to_string())
        .authorize_url(args.authorize_url.to_string())
        .notification_url(args.notification_url.to_string())
        .fail_open(args.fail_open)
        .build();

    config.save()?;

    let mut table = Table::new([&config]);
    table
        .with(Rotate::Left)
        .with(Style::modern())
        .with(Margin::new(4, 0, 0, 0));

    print_header();
    println!("{table}");

    Ok(())
}
