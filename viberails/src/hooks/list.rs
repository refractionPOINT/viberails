use anyhow::{Context, Result};
use log::info;

use crate::{
    logging::init_logging,
    providers::{Claude, LLmProviderTrait},
};

pub fn list_claude() -> Result<()> {
    let claude = Claude::new()?;

    let config = claude.config_file();

    info!("claude-code config file is @ {}", config.display());

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

pub fn list() -> Result<()> {
    init_logging::<String>(None).context("Unable to init logging")?;

    list_claude()
}
