use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use log::{info, warn};
use serde_json::{Value, json};

use crate::providers::{HookEntry, LLmProviderTrait};

pub struct Claude {
    self_program: String,
    settings: PathBuf,
}

impl Claude {
    pub fn with_custom_path<P>(self_program: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let config_dir = dirs::home_dir().ok_or_else(|| {
            anyhow!("Unable to determine home directory. Ensure HOME environment variable is set")
        })?;

        let claude_dir = config_dir.join(".claude");

        if !claude_dir.exists() {
            fs::create_dir_all(&claude_dir).with_context(|| {
                format!(
                    "Unable to create Claude config directory at {}",
                    claude_dir.display()
                )
            })?;
        }

        let settings = claude_dir.join("settings.json");

        let self_program = self_program
            .as_ref()
            .to_str()
            .ok_or_else(|| {
                anyhow!(
                    "Program path {:?} contains invalid UTF-8 characters",
                    self_program.as_ref()
                )
            })?
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

    pub(crate) fn install_into(&self, hook_type: &str, json: &mut Value) -> Result<()> {
        let root = json.as_object_mut().ok_or_else(|| {
            anyhow!(
                "Expected root of {} to be a JSON object",
                self.settings.display()
            )
        })?;

        let hooks_obj = root
            .entry("hooks")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                anyhow!(
                    "Expected 'hooks' field in {} to be an object",
                    self.settings.display()
                )
            })?;

        let hook_type_arr = hooks_obj
            .entry(hook_type)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| {
                anyhow!(
                    "Expected 'hooks.{hook_type}' field in {} to be an array",
                    self.settings.display()
                )
            })?;

        // Look for an existing entry with matcher "*"
        let wildcard_entry = hook_type_arr
            .iter_mut()
            .filter_map(|v| v.as_object_mut())
            .find(|obj| obj.get("matcher").and_then(|m| m.as_str()) == Some("*"));

        let our_hook = json!({
            "type": "command",
            "command": self.self_program
        });

        if let Some(entry) = wildcard_entry {
            let hooks_arr = entry
                .entry("hooks")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| {
                    anyhow!(
                        "Expected 'hooks' array in wildcard matcher for {hook_type} in {}",
                        self.settings.display()
                    )
                })?;

            // Check if already installed
            let already_installed = hooks_arr
                .iter()
                .any(|h| h.get("command").and_then(|c| c.as_str()) == Some(&self.self_program));

            if already_installed {
                warn!("{hook_type} already exist in {}", self.settings.display());
                return Ok(());
            }

            hooks_arr.insert(0, our_hook);
        } else {
            hook_type_arr.insert(
                0,
                json!({
                    "matcher": "*",
                    "hooks": [our_hook]
                }),
            );
        }

        Ok(())
    }

    pub(crate) fn uninstall_from(&self, hook_type: &str, json: &mut Value) -> Result<()> {
        let hooks_obj = json
            .as_object_mut()
            .and_then(|root| root.get_mut("hooks"))
            .and_then(|h| h.as_object_mut());

        let Some(hooks_obj) = hooks_obj else {
            warn!("No hooks found in {}", self.settings.display());
            return Ok(());
        };

        let Some(hook_type_arr) = hooks_obj.get_mut(hook_type).and_then(|v| v.as_array_mut())
        else {
            warn!("No {hook_type} hooks found in {}", self.settings.display());
            return Ok(());
        };

        // Find the wildcard entry
        let wildcard_entry = hook_type_arr
            .iter_mut()
            .filter_map(|v| v.as_object_mut())
            .find(|obj| obj.get("matcher").and_then(|m| m.as_str()) == Some("*"));

        let Some(entry) = wildcard_entry else {
            warn!(
                "No wildcard matcher found for {hook_type} in {}",
                self.settings.display()
            );
            return Ok(());
        };

        let Some(hooks_arr) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
            warn!(
                "No hooks array found in wildcard matcher for {hook_type} in {}",
                self.settings.display()
            );
            return Ok(());
        };

        // Remove our hook
        let original_len = hooks_arr.len();
        hooks_arr.retain(|h| h.get("command").and_then(|c| c.as_str()) != Some(&self.self_program));

        if hooks_arr.len() == original_len {
            warn!("{hook_type} hook not found in {}", self.settings.display());
        }

        Ok(())
    }
}

impl LLmProviderTrait for Claude {
    fn name(&self) -> &'static str {
        "claude-code"
    }

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
        let json_str =
            serde_json::to_string_pretty(&json).context("Failed to serialize Claude settings")?;

        let mut fd = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.settings)
            .with_context(|| format!("Unable to open {} for writing", self.settings.display()))?;

        fd.write_all(json_str.as_bytes())
            .with_context(|| format!("Failed to write to {}", self.settings.display()))?;

        Ok(())
    }

    fn uninstall(&self, hook_type: &str) -> anyhow::Result<()> {
        info!("Uninstalling {hook_type} in {}", self.settings.display());

        let data = fs::read_to_string(&self.settings)
            .with_context(|| format!("Unable to read {}", self.settings.display()))?;

        let mut json: Value = serde_json::from_str(&data)
            .with_context(|| format!("Unable to parse JSON data in {}", self.settings.display()))?;

        self.uninstall_from(hook_type, &mut json)
            .with_context(|| format!("Unable to update {}", self.settings.display()))?;

        let json_str =
            serde_json::to_string_pretty(&json).context("Failed to serialize Claude settings")?;

        let mut fd = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.settings)
            .with_context(|| format!("Unable to open {} for writing", self.settings.display()))?;

        fd.write_all(json_str.as_bytes())
            .with_context(|| format!("Failed to write to {}", self.settings.display()))?;

        Ok(())
    }

    fn list(&self) -> Result<Vec<HookEntry>> {
        let data = fs::read_to_string(&self.settings)
            .with_context(|| format!("Unable to read {}", self.settings.display()))?;

        let json: Value = serde_json::from_str(&data)
            .with_context(|| format!("Unable to parse JSON data in {}", self.settings.display()))?;

        let mut entries = Vec::new();

        let Some(hooks_obj) = json.get("hooks").and_then(|h| h.as_object()) else {
            return Ok(entries);
        };

        for (hook_type, hook_type_arr) in hooks_obj {
            let Some(hook_type_arr) = hook_type_arr.as_array() else {
                continue;
            };

            for matcher_entry in hook_type_arr {
                let matcher = matcher_entry
                    .get("matcher")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();

                let Some(hooks_arr) = matcher_entry.get("hooks").and_then(|h| h.as_array()) else {
                    continue;
                };

                for hook in hooks_arr {
                    if let Some(command) = hook.get("command").and_then(|c| c.as_str()) {
                        entries.push(HookEntry {
                            hook_type: hook_type.clone(),
                            matcher: matcher.clone(),
                            command: command.to_string(),
                        });
                    }
                }
            }
        }

        Ok(entries)
    }
}
