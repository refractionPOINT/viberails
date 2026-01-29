use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow};

pub const PROJECT_NAME: &str = env!("CARGO_PKG_NAME");

pub fn project_data_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_dir().ok_or_else(|| anyhow!("Couldn't find data dir"))?;

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
