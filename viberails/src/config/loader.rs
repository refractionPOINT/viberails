use std::{fs, io::Write, path::Path};

use anyhow::{Context, Result};
use bon::Builder;
use log::info;
use serde::{Deserialize, Serialize};
use tabled::{
    Table, Tabled,
    settings::{Margin, Rotate, Style},
};
use url::Url;
use uuid::Uuid;

use crate::common::{print_header, project_config_dir};

const CONFIG_FILE_NAME: &str = "config.json";
const DEF_LOGIN_URL: &str = "http://localhost:8000/login";
const DEF_AUTHORIZATION_URL: &str = "http://localhost:8000/dnr";
const DEF_NOTIFICATION_URL: &str = "http://localhost:8000/notify";

#[derive(clap::Args)]
pub struct ConfigureArgs {
    /// Authentication URL
    #[arg(long, default_value = DEF_LOGIN_URL)]
    login_url: Url,

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
pub struct UserConfig {
    pub login_url: String,
    pub authorize_url: String,
    pub notification_url: String,
    pub fail_open: bool,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            login_url: DEF_LOGIN_URL.to_string(),
            authorize_url: DEF_AUTHORIZATION_URL.to_string(),
            notification_url: DEF_NOTIFICATION_URL.to_string(),
            fail_open: true,
        }
    }
}

#[derive(Serialize, Deserialize, Tabled)]
pub struct Config {
    #[tabled(inline)]
    pub user: UserConfig,
    pub install_id: String,
}

impl Config {
    pub(crate) fn load_existing(config_file: &Path) -> Result<Self> {
        let config_string = fs::read_to_string(&config_file)
            .with_context(|| format!("Unable to read {}", config_file.display()))?;

        let config: Config = serde_json::from_str(&config_string)
            .context("Unable to deserialize configuration data")?;

        Ok(config)
    }

    fn create_new() -> Self {
        let user = UserConfig::default();
        let install_id = Uuid::new_v4().to_string();

        info!("install id: {install_id}");

        Self { user, install_id }
    }

    pub fn save(&self) -> Result<()> {
        let config_string =
            serde_json::to_string_pretty(self).context("Unable to serialize configuration data")?;

        let config_dir = project_config_dir()?;
        let config_file = config_dir.join(CONFIG_FILE_NAME);

        let mut fd = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&config_file)
            .with_context(|| format!("Unable to write {}", config_file.display()))?;

        fd.write_all(config_string.as_bytes()).with_context(|| {
            format!("Failed to write configuration to {}", config_file.display())
        })?;

        Ok(())
    }

    pub fn load() -> Result<Self> {
        let config_dir = project_config_dir()?;
        let config_file = config_dir.join("config.json");

        if config_file.exists() {
            Config::load_existing(&config_file)
        } else {
            //
            // doesn't exist yet
            //
            Ok(Config::create_new())
        }
    }
}

fn display_configuration(config: &Config) {
    let mut table = Table::new([config]);
    table
        .with(Rotate::Left)
        .with(Style::modern())
        .with(Margin::new(4, 0, 0, 0));

    print_header();
    println!("{table}");
}

////////////////////////////////////////////////////////////////////////////////
// PUBLIC
////////////////////////////////////////////////////////////////////////////////

pub fn uninstall_config() -> Result<()> {
    let config_dir = project_config_dir()?;
    let config_file = config_dir.join("config.json");

    if !config_dir.exists() {
        info!("{} doesn't exist", config_dir.display());
        return Ok(());
    }

    if config_file.exists() {
        info!("removing {}", config_file.display());

        fs::remove_file(&config_file)
            .with_context(|| format!("Unable to delete {}", config_file.display()))?;
    }

    info!("Deleting {}", config_dir.display());

    fs::remove_dir_all(&config_dir)
        .with_context(|| format!("Unable to delete {}", config_dir.display()))?;

    Ok(())
}

pub fn show_configuration() -> Result<()> {
    let config = Config::load()?;

    display_configuration(&config);

    Ok(())
}

pub fn configure(args: &ConfigureArgs) -> Result<()> {
    let user = UserConfig::builder()
        .login_url(args.login_url.to_string())
        .authorize_url(args.authorize_url.to_string())
        .notification_url(args.notification_url.to_string())
        .fail_open(args.fail_open)
        .build();

    let mut config = Config::load()?;

    config.user = user;

    config.save()?;

    display_configuration(&config);

    Ok(())
}
