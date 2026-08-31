use std::fmt::Display;
use std::io::Write;
use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use log::{info, warn};
use rust_embed::Embed;
use serde_json::Value;
use tempfile::NamedTempFile;

use crate::common::PROJECT_NAME;
use crate::hooks::{binary_location, safe_remove_file};
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
    /// `OpenCode` scans a fixed list of directories for plugins — its global
    /// config directory, the project `.opencode` directories, `~/.opencode` and
    /// `$OPENCODE_CONFIG_DIR` — and globs `{plugin,plugins}/*.{ts,js}` in each.
    /// The global one is the only machine-wide target, and it resolves through
    /// `xdg-basedir` to `$XDG_CONFIG_HOME/opencode`, falling back to
    /// `~/.config/opencode`, with no platform-specific branching at all.
    ///
    /// Consequently:
    /// - `dirs::config_dir()` is wrong: it returns `~/Library/Application
    ///   Support` on macOS and `%APPDATA%` on Windows, neither of which
    ///   `OpenCode` reads.
    /// - `OPENCODE_CONFIG` is deliberately ignored: it names a config *file*
    ///   that is merged into the config, and never moves plugin discovery.
    ///   Installing next to it would silently enforce nothing.
    /// - `OPENCODE_CONFIG_DIR` needs no handling: it *adds* a directory, and
    ///   the global config directory is always scanned as well.
    fn opencode_dir() -> Option<PathBuf> {
        Self::resolve_dir(
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            dirs::home_dir(),
        )
    }

    /// Resolution rules for [`opencode_dir`], separated from the environment so
    /// they can be tested without mutating process-wide state.
    pub(crate) fn resolve_dir(
        xdg_config_home: Option<PathBuf>,
        home: Option<PathBuf>,
    ) -> Option<PathBuf> {
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
    /// `OpenCode`'s own config file. Never written to; only read so that the
    /// dead `plugins` key earlier versions wrote there can be cleaned up.
    config_file: PathBuf,
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
            config_file: opencode_dir.as_ref().join("opencode.json"),
        }
    }

    /// The command the installed plugin invokes, for display in `list`.
    fn command_line(&self) -> String {
        format!("{} {CALLBACK_ARG}", self.program.display())
    }

    /// Contents of the plugin file if it exists and was generated by us.
    /// A file that merely shares the name must not be listed or removed.
    ///
    /// Anything we could not have written — a directory, a binary, a file that
    /// is not valid UTF-8 — reads as "not ours" rather than as an error, so
    /// `list` and `uninstall` report the same "leave it alone" outcome for it
    /// as they do for someone else's plugin.
    fn read_our_plugin(&self) -> Option<String> {
        let bytes = fs::read(&self.plugin_file).ok()?;
        let contents = String::from_utf8(bytes).ok()?;

        contents.contains(PLUGIN_MARKER).then_some(contents)
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

        // Substitute the name first: the binary path is outside our control, so
        // a path that happens to contain the name placeholder must not be
        // rewritten by the second pass.
        Ok(source
            .replace(NAME_PLACEHOLDER, PROJECT_NAME)
            .replace(BIN_PLACEHOLDER, &bin))
    }

    /// Write the plugin so that no reader can ever observe a partial file.
    ///
    /// An unparseable plugin is the worst failure mode available here: `OpenCode`
    /// declines to load it and carries on with no enforcement and no error, so
    /// a half-written file is indistinguishable from having no policy at all.
    /// The temp file is created inside the destination directory (rename cannot
    /// cross filesystems) with a fresh name, never a predictable one that could
    /// already be a symlink, and is fsynced before the rename so a crash cannot
    /// publish a zero-length plugin under the real name.
    fn write_plugin_atomically(&self, contents: &str, parent: &std::path::Path) -> Result<()> {
        let mut temp = NamedTempFile::new_in(parent).with_context(|| {
            format!(
                "Unable to create a temporary file in {} to install the plugin",
                parent.display()
            )
        })?;

        temp.write_all(contents.as_bytes())
            .and_then(|()| temp.as_file().sync_all())
            .with_context(|| format!("Unable to write {}", temp.path().display()))?;

        // A temp file is created 0600; the plugin has to stay world-readable
        // like the rest of OpenCode's config, or an OpenCode running under a
        // different uid with the same HOME silently loads no plugin.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o644))
                .with_context(|| {
                    format!("Unable to set permissions on {}", temp.path().display())
                })?;
        }

        // Dropping the NamedTempFile on any error above removes it, so a failed
        // install cannot litter the plugin directory.
        temp.persist(&self.plugin_file).map_err(|e| {
            anyhow!(
                "Unable to install plugin at {}: {}",
                self.plugin_file.display(),
                e.error
            )
        })?;

        // Best effort: makes the rename itself durable. A missed fsync here can
        // only lose the install, never corrupt it.
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }

        Ok(())
    }

    /// Remove the `plugins` key earlier versions wrote into `opencode.json`.
    ///
    /// That key was never read by `OpenCode` and is inert, but it is our litter
    /// and it names this project, so an operator reading their config would
    /// reasonably believe it is what enforces policy. Only our own entry is
    /// touched, and every failure is ignored: this is cleanup, and it must
    /// never be the reason an install fails.
    fn remove_legacy_config_entry(&self) {
        let Ok(data) = fs::read_to_string(&self.config_file) else {
            return;
        };

        let Ok(mut json) = serde_json::from_str::<Value>(&data) else {
            // A config with comments (jsonc) or hand-edited syntax errors is
            // not ours to rewrite.
            return;
        };

        let Some(plugins) = json
            .as_object_mut()
            .and_then(|root| root.get_mut("plugins"))
            .and_then(Value::as_object_mut)
        else {
            return;
        };

        if plugins.remove(PROJECT_NAME).is_none() {
            return;
        }

        // Drop the container too if we were the only thing in it, so the
        // cleanup leaves no trace of a key OpenCode does not define.
        if let Some(root) = json.as_object_mut()
            && root
                .get("plugins")
                .and_then(Value::as_object)
                .is_some_and(serde_json::Map::is_empty)
        {
            root.remove("plugins");
        }

        let Ok(serialized) = serde_json::to_string_pretty(&json) else {
            return;
        };

        if fs::write(&self.config_file, format!("{serialized}\n")).is_ok() {
            info!(
                "Removed the obsolete {PROJECT_NAME} plugins entry from {}",
                self.config_file.display()
            );
        }
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

        // Refuse to destroy a file we did not generate. `list` and `uninstall`
        // already decline to claim or delete one; overwriting it here would be
        // the one operation that does not respect that, and it is the
        // destructive one. Failing loudly is deliberate: the alternative is a
        // silent no-op reported as a successful install.
        if self.plugin_file.exists() && self.read_our_plugin().is_none() {
            return Err(anyhow!(
                "{} exists and was not generated by {PROJECT_NAME}; move it aside to install",
                self.plugin_file.display()
            ));
        }

        let parent = self
            .plugin_file
            .parent()
            .ok_or_else(|| anyhow!("Plugin path {} has no parent", self.plugin_file.display()))?;

        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Unable to create OpenCode plugin directory at {}",
                parent.display()
            )
        })?;

        let contents = self.render_plugin()?;
        self.write_plugin_atomically(&contents, parent)?;

        self.remove_legacy_config_entry();

        Ok(())
    }

    fn uninstall(&self, hook_type: &str) -> Result<()> {
        info!(
            "Uninstalling {hook_type} from {}",
            self.plugin_file.display()
        );

        // Never delete a file that is not ours, even though it occupies the name
        // we install under. `list` applies the same ownership test.
        if self.read_our_plugin().is_none() {
            warn!(
                "No {PROJECT_NAME} plugin at {}, leaving the path untouched",
                self.plugin_file.display()
            );
            return Ok(());
        }

        // safe_remove_file, rather than fs::remove_file, so that a symlink
        // planted at the plugin path is refused like everywhere else in the
        // uninstall path.
        safe_remove_file(&self.plugin_file)
    }

    fn list(&self) -> Result<Vec<HookEntry>> {
        let Some(contents) = self.read_our_plugin() else {
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
