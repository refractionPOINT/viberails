use std::fmt::Display;
use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use log::{info, warn};
use rust_embed::Embed;

use crate::common::PROJECT_NAME;
use crate::hooks::binary_location;
use crate::providers::discovery::{DiscoveryResult, ProviderDiscovery, ProviderFactory};
use crate::providers::{HookEntry, LLmProviderTrait};

#[derive(Embed)]
#[folder = "resources/opencode/"]
struct OpenCodeAssets;

/// Placeholder in the shipped plugin, replaced at install time with a JSON
/// string literal holding the binary path.
const BIN_PLACEHOLDER: &str = "__CALLBACK_BIN__";

/// Placeholder for the project name, so the generated plugin carries no
/// hardcoded branding and survives a rename of the crate.
const NAME_PLACEHOLDER: &str = "__PROJECT_NAME__";

/// Subcommand the installed plugin invokes.
const CALLBACK_ARG: &str = "opencode-callback";

/// Distinctive marker identifying a plugin file we generated. Checked before
/// listing or removing a file, so an unrelated plugin that merely happens to
/// mention the callback is never claimed as ours. Derived from the crate name
/// so it stays in step with the placeholder the plugin is rendered with.
const PLUGIN_MARKER: &str = concat!("@", env!("CARGO_PKG_NAME"), "-plugin");

/// Declaration holding the binary path in a generated plugin, used to report
/// the path the installed plugin actually calls rather than the expected one.
/// Deliberately project-neutral so a rename cannot break the read-back.
const BIN_CONST_PREFIX: &str = "const CALLBACK_BIN =";

/// `OpenCode` loads plugins from disk rather than from a hook command, so there
/// is a single logical hook: the plugin file itself.
/// See <https://opencode.ai/docs/plugins/>
pub const OPENCODE_HOOKS: &[&str] = &["plugin"];

/// Discovery implementation for `OpenCode`.
pub struct OpenCodeDiscovery;

impl OpenCodeDiscovery {
    /// Get the `OpenCode` config directory path.
    ///
    /// `OpenCode` resolves `$XDG_CONFIG_HOME/opencode`, falling back to
    /// `~/.config/opencode` on every platform (including macOS and Windows).
    /// `dirs::config_dir()` is deliberately not used: it returns
    /// `~/Library/Application Support` on macOS and `%APPDATA%` on Windows,
    /// neither of which `OpenCode` reads.
    fn opencode_dir() -> Option<PathBuf> {
        Self::resolve_dir(
            std::env::var_os("OPENCODE_CONFIG").map(PathBuf::from),
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            dirs::home_dir(),
        )
    }

    /// Resolution rules for [`opencode_dir`], separated from the environment so
    /// they can be tested without mutating process-wide state.
    pub(crate) fn resolve_dir(
        opencode_config: Option<PathBuf>,
        xdg_config_home: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Option<PathBuf> {
        // An explicit OPENCODE_CONFIG points at a config *file*; use its parent.
        if let Some(dir) = opencode_config
            .filter(|p| !p.as_os_str().is_empty())
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .filter(|p| !p.as_os_str().is_empty())
        {
            return Some(dir);
        }

        if let Some(dir) = xdg_config_home.filter(|p| !p.as_os_str().is_empty()) {
            return Some(dir.join("opencode"));
        }

        home.map(|h| h.join(".config").join("opencode"))
    }
}

impl ProviderDiscovery for OpenCodeDiscovery {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn discover(&self) -> DiscoveryResult {
        let opencode_dir = Self::opencode_dir();
        let detected = opencode_dir
            .as_ref()
            .is_some_and(|p| p.exists() && p.is_dir());
        let detected_path = opencode_dir.filter(|p| p.exists());

        DiscoveryResult {
            id: self.id(),
            display_name: self.display_name(),
            detected,
            detected_path,
            detection_hint: Some("Install OpenCode from https://opencode.ai/docs/cli/".into()),
            hooks_installed: false, // Will be set by discover_with_hooks_check
        }
    }

    fn supported_hooks(&self) -> &'static [&'static str] {
        OPENCODE_HOOKS
    }
}

impl ProviderFactory for OpenCodeDiscovery {
    fn create(&self) -> Result<Box<dyn LLmProviderTrait>> {
        Ok(Box::new(OpenCode::new()?))
    }
}

pub struct OpenCode {
    program: PathBuf,
    plugin_file: PathBuf,
}

impl OpenCode {
    pub fn new() -> Result<Self> {
        // Always use the installed binary location (~/.local/bin/viberails) rather than
        // current_exe(), so the plugin references a stable path regardless of where
        // viberails is run from.
        let exe = binary_location()?;
        Self::with_custom_path(exe)
    }

    pub fn with_custom_path<P: AsRef<std::path::Path>>(program: P) -> Result<Self> {
        let opencode_dir = OpenCodeDiscovery::opencode_dir()
            .ok_or_else(|| anyhow!("Unable to determine OpenCode config directory"))?;

        Ok(Self::with_dir(program, opencode_dir))
    }

    /// Build an instance rooted at an explicit `OpenCode` config directory.
    pub(crate) fn with_dir<P: AsRef<std::path::Path>, D: AsRef<std::path::Path>>(
        program: P,
        opencode_dir: D,
    ) -> Self {
        // OpenCode globs `{plugin,plugins}/*.{ts,js}` inside each config directory.
        let plugin_file = opencode_dir
            .as_ref()
            .join("plugin")
            .join(format!("{PROJECT_NAME}.js"));

        Self {
            program: program.as_ref().to_path_buf(),
            plugin_file,
        }
    }

    /// The command the installed plugin invokes, for display in `list`.
    fn command_line(&self) -> String {
        format!("{} {CALLBACK_ARG}", self.program.display())
    }

    /// Contents of the plugin file if it exists and was generated by us.
    /// A file that merely shares the name must not be listed or removed.
    fn read_our_plugin(&self) -> Result<Option<String>> {
        if !self.plugin_file.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(&self.plugin_file)
            .with_context(|| format!("Unable to read {}", self.plugin_file.display()))?;

        Ok(contents.contains(PLUGIN_MARKER).then_some(contents))
    }

    /// The command an already-installed plugin invokes, read back from the file.
    /// Reporting this rather than the expected path means a plugin left pointing
    /// at a moved binary shows up as stale instead of looking healthy.
    fn installed_command(contents: &str) -> Option<String> {
        let literal = contents
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix(BIN_CONST_PREFIX))?
            .trim()
            .strip_suffix(';')?
            .trim();

        let program: String = serde_json::from_str(literal).ok()?;

        Some(format!("{program} {CALLBACK_ARG}"))
    }

    /// Path of the plugin file this instance manages.
    #[cfg(test)]
    pub(crate) fn plugin_file(&self) -> &std::path::Path {
        &self.plugin_file
    }

    /// Render the embedded plugin with the real binary path substituted in.
    pub(crate) fn render_plugin(&self) -> Result<String> {
        let asset =
            OpenCodeAssets::get("plugin.js").ok_or_else(|| anyhow!("plugin.js is not embedded"))?;

        let source =
            std::str::from_utf8(&asset.data).context("Embedded plugin.js is not valid UTF-8")?;

        // Reject a non-UTF-8 path rather than lossily mangling it: a mangled
        // path yields a plugin pointing at a binary that does not exist, which
        // fails open on every tool call instead of failing loudly here.
        let program = self
            .program
            .to_str()
            .ok_or_else(|| anyhow!("Binary path is not valid UTF-8: {}", self.program.display()))?;

        // Emit a JSON string literal, which is also a valid JS string literal.
        // Hand-rolled quoting would miss newlines and control characters, and a
        // path carrying one would produce a plugin OpenCode cannot parse -- which
        // silently disables enforcement rather than failing loudly.
        let bin = serde_json::to_string(program)
            .context("Failed to encode binary path for the OpenCode plugin")?;

        Ok(source
            .replace(BIN_PLACEHOLDER, &bin)
            .replace(NAME_PLACEHOLDER, PROJECT_NAME))
    }
}

impl Display for OpenCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OpenCode")
    }
}

impl LLmProviderTrait for OpenCode {
    fn install(&self, hook_type: &str) -> Result<()> {
        info!("Installing {hook_type} at {}", self.plugin_file.display());

        if let Some(parent) = self.plugin_file.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Unable to create OpenCode plugin directory at {}",
                    parent.display()
                )
            })?;
        }

        let contents = self.render_plugin()?;

        // Write via a sibling temp file and rename. A truncating write that is
        // interrupted would leave a half-written plugin, which OpenCode cannot
        // parse -- and an unparseable plugin silently stops enforcing rather
        // than failing loudly. Rename replaces the destination on both Unix and
        // Windows, so the file is never observed partially written.
        let temp_file = self
            .plugin_file
            .with_extension(format!("js.{}.tmp", std::process::id()));

        // Any failure past this point must clear the temp file, or repeated
        // failures litter the plugin directory.
        let written = fs::write(&temp_file, contents)
            .with_context(|| format!("Unable to write {}", temp_file.display()))
            .and_then(|()| {
                fs::rename(&temp_file, &self.plugin_file).with_context(|| {
                    format!("Unable to install plugin at {}", self.plugin_file.display())
                })
            });

        if written.is_err() {
            let _ = fs::remove_file(&temp_file);
        }

        written
    }

    fn uninstall(&self, hook_type: &str) -> Result<()> {
        info!(
            "Uninstalling {hook_type} from {}",
            self.plugin_file.display()
        );

        if !self.plugin_file.exists() {
            warn!(
                "{PROJECT_NAME} plugin not found at {}",
                self.plugin_file.display()
            );
            return Ok(());
        }

        // Never delete a file that is not ours, even though it occupies the name
        // we install under. `list` applies the same ownership test.
        if self.read_our_plugin()?.is_none() {
            warn!(
                "{} is not a {PROJECT_NAME} plugin, leaving it in place",
                self.plugin_file.display()
            );
            return Ok(());
        }

        fs::remove_file(&self.plugin_file)
            .with_context(|| format!("Unable to remove {}", self.plugin_file.display()))?;

        Ok(())
    }

    fn list(&self) -> Result<Vec<HookEntry>> {
        let Some(contents) = self.read_our_plugin()? else {
            return Ok(Vec::new());
        };

        // Fall back to the expected command only if the generated declaration
        // cannot be read back, which means the file has been edited.
        let command = Self::installed_command(&contents).unwrap_or_else(|| self.command_line());

        Ok(vec![HookEntry {
            hook_type: "plugin".to_string(),
            matcher: PROJECT_NAME.to_string(),
            command,
        }])
    }
}
