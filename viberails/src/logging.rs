use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use env_logger::Target;

use crate::common::project_data_dir;

fn get_log_file_path(file_name: &str) -> Result<PathBuf> {
    let data_dir = project_data_dir()?;
    Ok(data_dir.join(file_name))
}

pub fn init_logging<S>(log_file_name: Option<S>) -> Result<()>
where
    S: AsRef<str>,
{
    let mut b = env_logger::builder();

    if let Some(file_name) = log_file_name {
        let log_file = get_log_file_path(file_name.as_ref())?;

        let fd = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_file)
            .with_context(|| format!("Unable to open {} for writing", log_file.display()))?;

        b.target(Target::Pipe(Box::new(fd)));
    }

    b.init();

    Ok(())
}
