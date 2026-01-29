use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use env_logger::Target;

use crate::common::project_data_dir;

pub struct Logging {
    file_name: Option<PathBuf>,
}

impl Logging {
    pub fn new() -> Self {
        Self { file_name: None }
    }

    pub fn with_file<P>(mut self, file_name: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.file_name = Some(file_name.into());
        self
    }

    pub fn start(&self) -> Result<()> {
        let mut b = env_logger::builder();

        if let Some(file_name) = &self.file_name {
            let log_file = get_log_file_path(file_name)?;

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
}

fn get_log_file_path(file_name: &Path) -> Result<PathBuf> {
    let data_dir = project_data_dir()?;
    Ok(data_dir.join(file_name))
}
