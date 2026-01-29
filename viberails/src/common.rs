use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow};

pub const PROJECT_NAME: &str = env!("CARGO_PKG_NAME");
pub const PROJECT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn print_header() {
    println!("{PROJECT_NAME} {PROJECT_VERSION}");
}

pub fn project_data_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_dir().ok_or_else(|| anyhow!("Unable to determine data directory. Ensure XDG_DATA_HOME or HOME environment variable is set"))?;

    let project_data_dir = data_dir.join(PROJECT_NAME);

    //
    // create the rootdir for our data is not there already
    //
    if !project_data_dir.exists() {
        fs::create_dir_all(&project_data_dir)
            .with_context(|| format!("Unable to create {}", project_data_dir.display()))?;
    }

    Ok(project_data_dir)
}

pub fn project_config_dir() -> Result<PathBuf> {
    let data_dir = dirs::config_dir().ok_or_else(|| anyhow!("Unable to determine config directory. Ensure XDG_CONFIG_HOME or HOME environment variable is set"))?;

    let project_data_dir = data_dir.join(PROJECT_NAME);

    //
    // create the rootdir for our data is not there already
    //
    if !project_data_dir.exists() {
        fs::create_dir_all(&project_data_dir)
            .with_context(|| format!("Unable to create {}", project_data_dir.display()))?;
    }

    Ok(project_data_dir)
}
