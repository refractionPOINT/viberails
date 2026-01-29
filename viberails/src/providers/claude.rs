use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::providers::LLmProviderTrait;

#[derive(Deserialize, Serialize, Clone, Debug)]
struct ClaudeHooks {
    #[serde(rename = "type")]
    hook_type: String,
    command: String,
}

impl ClaudeHooks {
    pub fn new(hook_type: &str, command: &str) -> Self {
        Self {
            hook_type: hook_type.to_string(),
            command: command.to_string(),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct ClaudeHook {
    matcher: String,
    hooks: Vec<ClaudeHooks>,
}

pub struct Claude {
    self_program: String,
    settings: PathBuf,
}

impl Claude {
    pub fn with_custom_path<P>(self_program: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let config_dir =
            dirs::home_dir().ok_or_else(|| anyhow!("Couldn't find config directory"))?;

        let claude_dir = config_dir.join(".claude");

        if !claude_dir.exists() {
            fs::create_dir_all(&claude_dir)?;
        }

        let settings = claude_dir.join("settings.json");

        let self_program = self_program
            .as_ref()
            .to_str()
            .ok_or_else(|| anyhow!("Invalid path"))?
            .to_string();

        Ok(Self {
            self_program,
            settings,
        })
    }

    pub fn new<P>(program: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        Claude::with_custom_path(program)
    }

    fn already_installed(&self, hooks: &[ClaudeHooks]) -> bool {
        for hook in hooks {
            if hook.command.eq(&self.self_program) {
                return true;
            }
        }
        false
    }

    pub(crate) fn install_into(&self, hook_type: &str, json: &mut Value) -> Result<()> {
        let hook_dict_value = json
            .as_object_mut()
            .ok_or_else(|| anyhow!("Expected JSON object"))?
            .entry("hooks")
            .or_insert_with(|| json!({}));

        let hooks_value = hook_dict_value
            .as_object_mut()
            .ok_or_else(|| anyhow!("Expected hooks to be an object"))?
            .entry(hook_type)
            .or_insert_with(|| json!([]));

        let mut hooks: Vec<ClaudeHook> = serde_json::from_value(hooks_value.clone())?;

        let our_hook = ClaudeHooks::new("command", &self.self_program);

        if let Some(entry) = hooks.iter_mut().find(|h| h.matcher == "*") {
            // not need to do anything
            if self.already_installed(&entry.hooks) {
                warn!("{hook_type} already exist in {}", self.settings.display());
                return Ok(());
            }
            entry.hooks.insert(0, our_hook);
        } else {
            hooks.insert(
                0,
                ClaudeHook {
                    matcher: "*".to_string(),
                    hooks: vec![our_hook],
                },
            );
        }

        *hooks_value = serde_json::to_value(hooks)?;

        Ok(())
    }
}

impl LLmProviderTrait for Claude {
    // Install
    fn install(&self, hook_type: &str) -> anyhow::Result<()> {
        info!("Installing {hook_type} in {}", self.settings.display());

        let data = fs::read_to_string(&self.settings)
            .with_context(|| format!("Unable to read {}", self.settings.display()))?;

        let mut json: Value = serde_json::from_str(&data)
            .with_context(|| format!("Unable to parse JSON data in {}", self.settings.display()))?;

        self.install_into(hook_type, &mut json)
            .with_context(|| format!("Unable to update {}", self.settings.display()))?;

        //
        // this should now be updated. Write it back to the file
        //

        let json_str = serde_json::to_string_pretty(&json)?;

        let mut fd = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.settings)?;

        fd.write_all(json_str.as_bytes())?;

        Ok(())
    }

    fn uninstall(&self, hook_type: &str) -> anyhow::Result<()> {
        info!("Uninstalling {hook_type} in {}", self.settings.display());
        bail!("Not Implemented")
    }

    /*
    fn config_file(&self) -> &Path {
        &self.settings
    }
    */
}
