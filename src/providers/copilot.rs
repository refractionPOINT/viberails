use std::fmt::Display;
use std::process::Command;
use std::{env, fs, io::Write, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};
use serde_json::{Value, json};

use crate::common::EXECUTABLE_NAME;
use crate::hooks::binary_location;
use crate::providers::discovery::{DiscoveryResult, ProviderDiscovery, ProviderFactory};
use crate::providers::{HookAnswer, HookDecision, HookEntry, LLmProviderTrait};

use std::io::BufWriter;
use std::io::Stdout;

/// Supported hooks for GitHub Copilot CLI
/// Based on <https://docs.github.com/en/copilot/how-tos/copilot-cli/use-hooks>
///
/// Copilot CLI hooks are per-project, stored at `.github/hooks/hooks.json`
/// relative to the git repository root (or current working directory).
pub const COPILOT_HOOKS: &[&str] = &["preToolUse", "userPromptSubmitted", "postToolUse"];

/// Discovery implementation for GitHub Copilot CLI.
pub struct CopilotDiscovery;

impl CopilotDiscovery {
    /// Get the Copilot config directory path.
    ///
    /// Note: Copilot documents `XDG_CONFIG_HOME` as an override, but it defaults
    /// to `$HOME/.copilot` (non-standard XDG usage). We don't use `XDG_CONFIG_HOME`
    /// for discovery because it's commonly set to `~/.config` on Linux, which would
    /// cause false positive detection on every Linux system.
    fn copilot_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".copilot"))
    }
}

impl ProviderDiscovery for CopilotDiscovery {
    fn id(&self) -> &'static str {
        "copilot"
    }

    fn display_name(&self) -> &'static str {
        "GitHub Copilot CLI"
    }

    fn discover(&self) -> DiscoveryResult {
        let copilot_dir = Self::copilot_dir();
        let detected = copilot_dir
            .as_ref()
            .is_some_and(|p| p.exists() && p.is_dir());
        let detected_path = copilot_dir.filter(|p| p.exists());

        DiscoveryResult {
            id: self.id(),
            display_name: self.display_name(),
            detected,
            detected_path,
            detection_hint: Some(
                "Install GitHub Copilot CLI from https://docs.github.com/en/copilot/using-github-copilot/using-github-copilot-in-the-command-line".into(),
            ),
            hooks_installed: false, // Will be set by discover_with_hooks_check
        }
    }

    fn supported_hooks(&self) -> &'static [&'static str] {
        COPILOT_HOOKS
    }
}

impl ProviderFactory for CopilotDiscovery {
    fn create(&self) -> Result<Box<dyn LLmProviderTrait>> {
        Ok(Box::new(Copilot::new()?))
    }
}

pub struct Copilot {
    command_line: String,
    hooks_file: PathBuf,
}

impl Copilot {
    pub fn new() -> Result<Self> {
        // Always use the installed binary location (~/.local/bin/viberails) rather than
        // current_exe(), so the hook command is consistent regardless of where viberails
        // is run from.
        let exe = binary_location()?;
        Self::with_custom_path(exe)
    }

    pub fn with_custom_path<P: AsRef<std::path::Path>>(program: P) -> Result<Self> {
        let command_line = format!("{} copilot-callback", program.as_ref().display());

        // Copilot CLI hooks are per-project at .github/hooks/hooks.json
        // Use git root if available, otherwise fall back to CWD
        let project_root = Self::find_git_root()
            .or_else(|| env::current_dir().ok())
            .ok_or_else(|| {
                anyhow!("Unable to determine project root. Run from within a git repository")
            })?;

        let hooks_file = project_root.join(".github").join("hooks").join("hooks.json");
        debug!("Copilot hooks file: {}", hooks_file.display());

        Ok(Self {
            command_line,
            hooks_file,
        })
    }

    /// Create a Copilot provider with explicit program path and hooks file.
    /// Used for testing only.
    #[cfg(test)]
    pub fn with_test_paths<P: AsRef<std::path::Path>>(
        program: P,
        hooks_file: PathBuf,
    ) -> Self {
        let command_line = format!("{} copilot-callback", program.as_ref().display());

        Self {
            command_line,
            hooks_file,
        }
    }

    /// Find the git repository root from the current working directory.
    /// Returns None if not inside a git repository.
    fn find_git_root() -> Option<PathBuf> {
        Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .ok()
                    .map(|s| PathBuf::from(s.trim()))
            })
    }

    /// Ensure the hooks file exists, creating it with default structure if needed.
    fn ensure_hooks_exist(&self) -> Result<()> {
        if let Some(parent) = self.hooks_file.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Unable to create hooks directory at {}",
                    parent.display()
                )
            })?;
        }

        if !self.hooks_file.exists() {
            let default = json!({
                "version": 1,
                "hooks": {}
            });
            let json_str = serde_json::to_string_pretty(&default)
                .context("Failed to serialize default Copilot hooks")?;
            fs::write(&self.hooks_file, json_str).with_context(|| {
                format!(
                    "Unable to create Copilot hooks file at {}",
                    self.hooks_file.display()
                )
            })?;
        }

        Ok(())
    }

    pub(crate) fn install_into(&self, hook_type: &str, json: &mut Value) -> Result<()> {
        let root = json.as_object_mut().ok_or_else(|| {
            anyhow!(
                "Expected root of {} to be a JSON object",
                self.hooks_file.display()
            )
        })?;

        // Ensure version field exists
        root.entry("version").or_insert(json!(1));

        let hooks_obj = root
            .entry("hooks")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                anyhow!(
                    "Expected 'hooks' field in {} to be an object",
                    self.hooks_file.display()
                )
            })?;

        let hook_type_arr = hooks_obj
            .entry(hook_type)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| {
                anyhow!(
                    "Expected 'hooks.{hook_type}' field in {} to be an array",
                    self.hooks_file.display()
                )
            })?;

        // Copilot uses `bash` field (not `command`) and has no `matcher` field
        // Check if already installed by matching the `bash` field
        let already_installed = hook_type_arr
            .iter()
            .any(|h| h.get("bash").and_then(|c| c.as_str()) == Some(&self.command_line));

        if already_installed {
            warn!(
                "{hook_type} already exists in {}",
                self.hooks_file.display()
            );
            return Ok(());
        }

        // Insert our hook at the beginning
        // Copilot format: { "type": "command", "bash": "<cmd>", "comment": "..." }
        hook_type_arr.insert(
            0,
            json!({
                "type": "command",
                "bash": &self.command_line,
                "comment": "viberails security hook"
            }),
        );

        Ok(())
    }

    pub(crate) fn uninstall_from(&self, hook_type: &str, json: &mut Value) {
        let hooks_obj = json
            .as_object_mut()
            .and_then(|root| root.get_mut("hooks"))
            .and_then(|h| h.as_object_mut());

        let Some(hooks_obj) = hooks_obj else {
            warn!("No hooks found in {}", self.hooks_file.display());
            return;
        };

        let Some(hook_type_arr) = hooks_obj.get_mut(hook_type).and_then(|v| v.as_array_mut())
        else {
            warn!(
                "No {hook_type} hooks found in {}",
                self.hooks_file.display()
            );
            return;
        };

        // Remove our hook by matching executables ending with EXECUTABLE_NAME in the `bash` field
        let original_len = hook_type_arr.len();
        hook_type_arr.retain(|h| {
            let Some(cmd) = h.get("bash").and_then(|c| c.as_str()) else {
                return true;
            };
            !cmd.split_whitespace()
                .next()
                .is_some_and(|exe| exe.ends_with(EXECUTABLE_NAME))
        });

        if hook_type_arr.len() == original_len {
            warn!(
                "{hook_type} hook not found in {}",
                self.hooks_file.display()
            );
        }
    }
}

impl Display for Copilot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Copilot")
    }
}

impl LLmProviderTrait for Copilot {
    fn install(&self, hook_type: &str) -> Result<()> {
        info!("Installing {hook_type} in {}", self.hooks_file.display());

        self.ensure_hooks_exist()?;

        let data = fs::read_to_string(&self.hooks_file)
            .with_context(|| format!("Unable to read {}", self.hooks_file.display()))?;

        let mut json: Value = serde_json::from_str(&data).with_context(|| {
            format!("Unable to parse JSON data in {}", self.hooks_file.display())
        })?;

        self.install_into(hook_type, &mut json)
            .with_context(|| format!("Unable to update {}", self.hooks_file.display()))?;

        let json_str =
            serde_json::to_string_pretty(&json).context("Failed to serialize Copilot hooks")?;

        let mut fd = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.hooks_file)
            .with_context(|| format!("Unable to open {} for writing", self.hooks_file.display()))?;

        fd.write_all(json_str.as_bytes())
            .with_context(|| format!("Failed to write to {}", self.hooks_file.display()))?;

        Ok(())
    }

    fn uninstall(&self, hook_type: &str) -> Result<()> {
        info!(
            "Uninstalling {hook_type} from {}",
            self.hooks_file.display()
        );

        let data = fs::read_to_string(&self.hooks_file)
            .with_context(|| format!("Unable to read {}", self.hooks_file.display()))?;

        let mut json: Value = serde_json::from_str(&data).with_context(|| {
            format!("Unable to parse JSON data in {}", self.hooks_file.display())
        })?;

        self.uninstall_from(hook_type, &mut json);

        let json_str =
            serde_json::to_string_pretty(&json).context("Failed to serialize Copilot hooks")?;

        let mut fd = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.hooks_file)
            .with_context(|| format!("Unable to open {} for writing", self.hooks_file.display()))?;

        fd.write_all(json_str.as_bytes())
            .with_context(|| format!("Failed to write to {}", self.hooks_file.display()))?;

        Ok(())
    }

    fn list(&self) -> Result<Vec<HookEntry>> {
        let data = fs::read_to_string(&self.hooks_file)
            .with_context(|| format!("Unable to read {}", self.hooks_file.display()))?;

        let json: Value = serde_json::from_str(&data).with_context(|| {
            format!("Unable to parse JSON data in {}", self.hooks_file.display())
        })?;

        let mut entries = Vec::new();

        let Some(hooks_obj) = json.get("hooks").and_then(|h| h.as_object()) else {
            return Ok(entries);
        };

        for (hook_type, hook_arr) in hooks_obj {
            let Some(hook_arr) = hook_arr.as_array() else {
                continue;
            };

            for hook in hook_arr {
                // Copilot uses `bash` field, not `command`
                if let Some(bash_cmd) = hook.get("bash").and_then(|c| c.as_str()) {
                    entries.push(HookEntry {
                        hook_type: hook_type.clone(),
                        matcher: "*".to_string(),
                        command: bash_cmd.to_string(),
                    });
                }
            }
        }

        Ok(entries)
    }

    /// Override `write_answer` for Copilot's permission decision format.
    /// Copilot CLI expects `permissionDecision`/`permissionDecisionReason` instead
    /// of the default `decision`/`reason` format used by Claude Code and Cursor.
    fn write_answer(&self, writer: &mut BufWriter<Stdout>, answer: HookAnswer) -> Result<()> {
        match answer.decision {
            HookDecision::Approve => {
                // No output on approve -- exit 0 with no output means "allow"
                info!("decision: approve (no output, exit 0)");
                Ok(())
            }
            HookDecision::Block => {
                // Copilot expects: {"permissionDecision":"deny","permissionDecisionReason":"..."}
                let copilot_response = json!({
                    "permissionDecision": "deny",
                    "permissionDecisionReason": answer.reason.as_deref()
                        .unwrap_or("Blocked by viberails policy")
                });

                let resp_string = serde_json::to_string(&copilot_response)
                    .context("Failed to serialize Copilot hook response")?;

                info!("decision json: {resp_string}");

                writer
                    .write_all(resp_string.as_bytes())
                    .context("Failed to write hook response to stdout")?;
                writer.flush().context("Failed to flush hook response")?;

                Ok(())
            }
        }
    }
}
