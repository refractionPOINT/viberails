#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use tempfile::TempDir;

use crate::common::PROJECT_NAME;
use crate::providers::opencode::{OPENCODE_HOOKS, OpenCode, OpenCodeDiscovery};
use crate::providers::{LLmProviderTrait, ProviderDiscovery};

const PROGRAM: &str = "/usr/bin/test-program";

/// The generated declaration `list` reads the installed path back from.
/// Kept in one place so a rename in the plugin fails loudly here.
const BIN_DECL: &str = "const CALLBACK_BIN =";

/// Placeholders the shipped plugin is rendered from. Naming a placeholder that
/// no longer exists would make the "not present after render" checks vacuous.
const BIN_PLACEHOLDER: &str = "__CALLBACK_BIN__";
const NAME_PLACEHOLDER: &str = "__PROJECT_NAME__";

fn make_opencode(dir: &TempDir) -> OpenCode {
    OpenCode::with_dir(PROGRAM, dir.path())
}

/// Write an unrelated plugin at the path we install to.
fn write_foreign_plugin(opencode: &OpenCode) {
    let path = opencode.plugin_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "export const other = async () => ({})").unwrap();
}

#[test]
fn test_resolve_dir_prefers_xdg_config_home() {
    // OpenCode's global config dir resolves through xdg-basedir, so an
    // exported XDG_CONFIG_HOME moves the directory it globs plugins from.
    let resolved = OpenCodeDiscovery::resolve_dir(
        Some(PathBuf::from("/xdg")),
        Some(PathBuf::from("/home/eric")),
    );

    assert_eq!(resolved, Some(PathBuf::from("/xdg/opencode")));
}

#[test]
fn test_resolve_dir_falls_back_to_dot_config_under_home() {
    // OpenCode uses ~/.config/opencode on every platform, including macOS and
    // Windows, so this must not follow the OS-native config location.
    let resolved = OpenCodeDiscovery::resolve_dir(None, Some(PathBuf::from("/home/eric")));

    assert_eq!(resolved, Some(PathBuf::from("/home/eric/.config/opencode")));
}

#[test]
fn test_resolve_dir_ignores_empty_env_values() {
    // An exported-but-empty variable must not win over the home fallback.
    let resolved =
        OpenCodeDiscovery::resolve_dir(Some(PathBuf::new()), Some(PathBuf::from("/home/eric")));

    assert_eq!(resolved, Some(PathBuf::from("/home/eric/.config/opencode")));
}

#[test]
fn test_resolve_dir_without_home_is_none() {
    assert_eq!(OpenCodeDiscovery::resolve_dir(None, None), None);
}

#[test]
fn test_resolve_dir_does_not_depend_on_opencode_config() {
    // OPENCODE_CONFIG names a config *file* that OpenCode merges; it does not
    // move plugin discovery, which only ever scans the directories in
    // ConfigPaths.directories(). Installing beside that file would report
    // success and enforce nothing, so the resolver must not accept it as an
    // input at all -- this asserts the signature, and so fails to compile if
    // an OPENCODE_CONFIG branch is ever reintroduced.
    let resolve: fn(Option<PathBuf>, Option<PathBuf>) -> Option<PathBuf> =
        OpenCodeDiscovery::resolve_dir;

    assert_eq!(
        resolve(None, Some(PathBuf::from("/home/eric"))),
        Some(PathBuf::from("/home/eric/.config/opencode"))
    );
}

#[test]
fn test_plugin_path_matches_opencode_glob() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    // OpenCode globs `{plugin,plugins}/*.{ts,js}` under its config directory.
    let expected = dir.path().join("plugin").join(format!("{PROJECT_NAME}.js"));
    assert_eq!(opencode.plugin_file(), expected);
}

#[test]
fn test_render_substitutes_binary_path() {
    let dir = TempDir::new().unwrap();
    let rendered = make_opencode(&dir).render_plugin().unwrap();

    assert!(!rendered.contains(BIN_PLACEHOLDER));
    assert!(rendered.contains(PROGRAM));
    // The plugin must invoke the callback subcommand.
    assert!(rendered.contains("opencode-callback"));
}

#[test]
fn test_render_escapes_windows_style_paths() {
    let dir = TempDir::new().unwrap();
    let opencode = OpenCode::with_dir(r"C:\Program Files\viberails.exe", dir.path());

    let rendered = opencode.render_plugin().unwrap();

    // Backslashes must be escaped so the emitted JS string literal stays valid.
    assert!(rendered.contains(r"C:\\Program Files\\viberails.exe"));
}

#[test]
fn test_render_escapes_control_characters_in_path() {
    let dir = TempDir::new().unwrap();
    // A path carrying a newline must not break out of the string literal: that
    // would yield a plugin OpenCode cannot parse, silently ending enforcement.
    let opencode = OpenCode::with_dir("/tmp/x\nimport('evil');//", dir.path());

    let rendered = opencode.render_plugin().unwrap();

    let line = rendered
        .lines()
        .find(|l| l.contains(BIN_DECL))
        .expect("binary constant is present");

    // The whole path stays on one line, escaped.
    assert!(line.contains(r"\n"), "newline must be escaped: {line}");
    assert!(line.ends_with(';'), "statement must terminate: {line}");
    assert!(!rendered.contains("\nimport('evil');"));
}

#[test]
fn test_render_substitutes_only_the_binary_constant() {
    let dir = TempDir::new().unwrap();
    let rendered = make_opencode(&dir).render_plugin().unwrap();

    // The placeholder must not survive anywhere, and must not have been
    // substituted into prose elsewhere in the file.
    assert!(!rendered.contains(BIN_PLACEHOLDER));
    assert_eq!(rendered.matches(PROGRAM).count(), 1);
}

#[test]
fn test_render_carries_no_hardcoded_project_name() {
    let dir = TempDir::new().unwrap();
    let rendered = make_opencode(&dir).render_plugin().unwrap();

    // Every reference to the project must come from the crate name, so a
    // rename cannot leave stale branding in the generated plugin.
    assert!(!rendered.contains(NAME_PLACEHOLDER));
    assert!(rendered.contains(&format!("@{PROJECT_NAME}-plugin")));
    assert!(rendered.contains(&format!("const PLUGIN_NAME = \"{PROJECT_NAME}\"")));
}

#[test]
fn test_render_rejects_non_utf8_binary_path() {
    let dir = TempDir::new().unwrap();

    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // Lossy conversion would emit a mangled path, producing a plugin that
        // fails open on every call rather than failing loudly at install time.
        let bad = OsStr::from_bytes(b"/tmp/\xff\xfeviberails");
        let opencode = OpenCode::with_dir(bad, dir.path());

        assert!(opencode.render_plugin().is_err());
    }

    #[cfg(not(unix))]
    let _ = dir;
}

#[test]
fn test_install_writes_plugin_file() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    let path = opencode.plugin_file();
    assert!(path.exists());

    let contents = std::fs::read_to_string(path).unwrap();
    assert!(contents.contains("opencode-callback"));
    assert!(contents.contains("tool.execute.before"));
}

#[test]
fn test_install_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();
    let first = std::fs::read_to_string(opencode.plugin_file()).unwrap();

    opencode.install(OPENCODE_HOOKS[0]).unwrap();
    let second = std::fs::read_to_string(opencode.plugin_file()).unwrap();

    assert_eq!(first, second);
    assert_eq!(opencode.list().unwrap().len(), 1);
}

#[test]
fn test_list_reports_installed_plugin() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    assert!(opencode.list().unwrap().is_empty());

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    let entries = opencode.list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].hook_type, "plugin");
    assert_eq!(entries[0].matcher, PROJECT_NAME);
    assert_eq!(entries[0].command, format!("{PROGRAM} opencode-callback"));
}

#[test]
fn test_list_reports_the_path_the_installed_plugin_actually_calls() {
    let dir = TempDir::new().unwrap();

    // Install pointing at one binary, then ask a differently-configured
    // instance to list: the stale installed path must be what is reported.
    OpenCode::with_dir("/old/location/viberails", dir.path())
        .install(OPENCODE_HOOKS[0])
        .unwrap();

    let entries = OpenCode::with_dir("/new/location/viberails", dir.path())
        .list()
        .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].command, "/old/location/viberails opencode-callback",
        "list must report the stale installed path, not the expected one"
    );
}

#[test]
fn test_list_falls_back_when_declaration_is_unreadable() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    // Keep our marker but mangle the generated declaration. Install under a
    // different path so the fallback is distinguishable from a successful
    // read-back -- otherwise this test would pass vacuously.
    let installed = OpenCode::with_dir("/old/location/viberails", dir.path());
    installed.install(OPENCODE_HOOKS[0]).unwrap();

    let contents = std::fs::read_to_string(installed.plugin_file()).unwrap();
    assert!(
        contents.contains(BIN_DECL),
        "declaration must exist to remove"
    );

    let mangled: String = contents
        .lines()
        .filter(|l| !l.trim().starts_with(BIN_DECL))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(installed.plugin_file(), mangled).unwrap();

    // With no declaration to read, `list` falls back to the expected command.
    let entries = opencode.list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, format!("{PROGRAM} opencode-callback"));
}

#[test]
fn test_list_ignores_plugin_without_our_marker() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    // Mentions the callback but was not generated by us.
    let path = opencode.plugin_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "// mentions opencode-callback but is not ours\n").unwrap();

    assert!(opencode.list().unwrap().is_empty());
}

#[test]
fn test_install_leaves_no_temp_files_behind() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    let stray: Vec<_> = std::fs::read_dir(opencode.plugin_file().parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();

    assert!(stray.is_empty(), "atomic write left temp files: {stray:?}");
}

#[test]
fn test_install_replaces_an_existing_plugin_atomically() {
    let dir = TempDir::new().unwrap();

    OpenCode::with_dir("/old/location/viberails", dir.path())
        .install(OPENCODE_HOOKS[0])
        .unwrap();

    let opencode = make_opencode(&dir);
    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    let contents = std::fs::read_to_string(opencode.plugin_file()).unwrap();
    assert!(contents.contains(PROGRAM));
    assert!(!contents.contains("/old/location/viberails"));
}

#[test]
fn test_list_ignores_foreign_plugin_with_same_name() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    write_foreign_plugin(&opencode);

    assert!(opencode.list().unwrap().is_empty());
}

#[test]
fn test_uninstall_leaves_foreign_plugin_with_same_name() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    write_foreign_plugin(&opencode);

    opencode.uninstall(OPENCODE_HOOKS[0]).unwrap();

    // Uninstall must not delete a file we did not install, even though it
    // occupies the name we install under.
    assert!(opencode.plugin_file().exists());
}

#[test]
fn test_uninstall_removes_plugin_file() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();
    assert!(opencode.plugin_file().exists());

    opencode.uninstall(OPENCODE_HOOKS[0]).unwrap();

    assert!(!opencode.plugin_file().exists());
    assert!(opencode.list().unwrap().is_empty());
}

#[test]
fn test_uninstall_when_not_installed_is_ok() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    // Uninstalling a plugin that was never installed must not error.
    opencode.uninstall(OPENCODE_HOOKS[0]).unwrap();
}

#[test]
fn test_uninstall_leaves_other_plugins_alone() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    let sibling = opencode.plugin_file().parent().unwrap().join("other.js");
    std::fs::write(&sibling, "export const other = async () => ({})").unwrap();

    opencode.uninstall(OPENCODE_HOOKS[0]).unwrap();

    assert!(!opencode.plugin_file().exists());
    assert!(sibling.exists());
}

#[test]
fn test_install_refuses_to_clobber_foreign_plugin() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    write_foreign_plugin(&opencode);
    let before = std::fs::read_to_string(opencode.plugin_file()).unwrap();

    // Overwriting is the destructive operation, so it must respect the same
    // ownership test that `list` and `uninstall` apply -- and say so, rather
    // than reporting a successful install of a plugin it did not write.
    let err = opencode.install(OPENCODE_HOOKS[0]).unwrap_err();
    assert!(
        err.to_string().contains("not generated by"),
        "unexpected error: {err}"
    );

    assert_eq!(
        std::fs::read_to_string(opencode.plugin_file()).unwrap(),
        before
    );
}

#[test]
fn test_install_replaces_our_own_stale_plugin() {
    let dir = TempDir::new().unwrap();

    // The ownership guard must not stand in the way of a reinstall pointing at
    // a new binary location, which is the common upgrade path.
    OpenCode::with_dir("/old/location/viberails", dir.path())
        .install(OPENCODE_HOOKS[0])
        .unwrap();

    let opencode = make_opencode(&dir);
    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    assert_eq!(
        opencode.list().unwrap()[0].command,
        format!("{PROGRAM} opencode-callback")
    );
}

#[test]
fn test_plugin_handles_stdin_write_errors() {
    let dir = TempDir::new().unwrap();
    let rendered = make_opencode(&dir).render_plugin().unwrap();

    // The callback exits without draining stdin whenever it bails early (an
    // unauthorized org, a cloud that fails to initialize). Node delivers that
    // as an 'error' event on the stream, and an 'error' event with no listener
    // is an uncaught exception in OpenCode's own process -- a try/catch around
    // write() does not see it. There is no JS test harness in the tree, so
    // this is the regression guard for that listener.
    assert!(
        rendered.contains(r#"child.stdin.on("error""#),
        "the plugin must listen for stdin stream errors"
    );
}

#[test]
fn test_render_substitutes_name_before_binary_path() {
    let dir = TempDir::new().unwrap();

    // The binary path comes from the environment, so it must be substituted
    // last: a path containing the name placeholder would otherwise be rewritten
    // by the name pass into a path that does not exist.
    let program = format!("/home/{NAME_PLACEHOLDER}/.local/bin/{PROJECT_NAME}");
    let opencode = OpenCode::with_dir(&program, dir.path());

    let rendered = opencode.render_plugin().unwrap();

    assert!(
        rendered.contains(&program),
        "binary path was rewritten by the name substitution"
    );
}

#[test]
fn test_non_utf8_file_at_plugin_path_is_left_alone() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    // A file we could not have written is "not ours", not an error: `list` and
    // `uninstall` must report the same leave-it-alone outcome they do for
    // someone else's plugin instead of failing the whole provider.
    let path = opencode.plugin_file();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, [0xff, 0xfe, 0x00, 0x01]).unwrap();

    assert!(opencode.list().unwrap().is_empty());
    opencode.uninstall(OPENCODE_HOOKS[0]).unwrap();
    assert!(path.exists(), "a file we did not write must survive");
}

#[test]
fn test_install_does_not_create_an_opencode_config() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    // Enforcement needs no config change at all; creating opencode.json would
    // be writing to a file OpenCode owns for no reason.
    assert!(!dir.path().join("opencode.json").exists());
}

#[test]
fn test_install_removes_the_legacy_plugins_entry() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    // What earlier versions wrote: a `plugins` key OpenCode never reads.
    let config = dir.path().join("opencode.json");
    std::fs::write(
        &config,
        format!(
            r#"{{
  "model": "gpt-4",
  "plugins": {{
    "{PROJECT_NAME}": {{ "enabled": true, "command": "/old opencode-callback" }},
    "someone-else": {{ "enabled": true }}
  }}
}}"#
        ),
    )
    .unwrap();

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();

    assert!(json["plugins"].get(PROJECT_NAME).is_none(), "our litter");
    assert!(json["plugins"]["someone-else"].is_object(), "not ours");
    assert_eq!(json["model"], "gpt-4", "unrelated config preserved");
}

#[test]
fn test_install_drops_the_plugins_key_when_it_becomes_empty() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    let config = dir.path().join("opencode.json");
    std::fs::write(
        &config,
        format!(r#"{{ "plugins": {{ "{PROJECT_NAME}": {{ "enabled": true }} }} }}"#),
    )
    .unwrap();

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();

    assert!(
        json.get("plugins").is_none(),
        "an empty container is still litter: {json}"
    );
}

#[test]
fn test_install_leaves_a_config_it_cannot_parse_untouched() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    // OpenCode also reads opencode.jsonc; a config carrying comments or a hand
    // edit is not ours to rewrite, and must not fail the install either.
    let config = dir.path().join("opencode.json");
    let original = "{\n  // a comment JSON cannot parse\n  \"model\": \"gpt-4\"\n}";
    std::fs::write(&config, original).unwrap();

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    assert_eq!(opencode.list().unwrap().len(), 1);
}

#[test]
fn test_install_leaves_a_config_without_our_entry_untouched() {
    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    let config = dir.path().join("opencode.json");
    let original = "{\n  \"plugins\": {\n    \"someone-else\": {}\n  }\n}";
    std::fs::write(&config, original).unwrap();

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn test_install_leaves_the_plugin_world_readable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let opencode = make_opencode(&dir);

    opencode.install(OPENCODE_HOOKS[0]).unwrap();

    // The plugin is written through a temp file, which is created 0600. An
    // OpenCode running under another uid with the same HOME would silently
    // load nothing, so the mode has to be widened before the rename.
    let mode = std::fs::metadata(opencode.plugin_file())
        .unwrap()
        .permissions()
        .mode();

    assert_eq!(mode & 0o777, 0o644, "unexpected mode {:o}", mode & 0o777);
}

#[cfg(unix)]
#[test]
fn test_uninstall_refuses_a_symlink_at_the_plugin_path() {
    let dir = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();

    // Our own plugin, installed somewhere else, then linked to from the path we
    // manage: following the link would delete a file outside the directory we
    // were asked to clean.
    let real = OpenCode::with_dir(PROGRAM, elsewhere.path());
    real.install(OPENCODE_HOOKS[0]).unwrap();

    let opencode = make_opencode(&dir);
    let link = opencode.plugin_file();
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(real.plugin_file(), link).unwrap();

    assert!(opencode.uninstall(OPENCODE_HOOKS[0]).is_err());
    assert!(
        real.plugin_file().exists(),
        "the symlink target must survive"
    );
    assert!(link.is_symlink(), "the symlink itself must survive");
}

// Discovery tests

#[test]
fn test_opencode_discovery_id() {
    // The id is what `--providers opencode` and ProviderRegistry::get match on.
    assert_eq!(OpenCodeDiscovery.id(), "opencode");
}

#[test]
fn test_opencode_discovery_display_name() {
    assert_eq!(OpenCodeDiscovery.display_name(), "OpenCode");
}

#[test]
fn test_opencode_discovery_supported_hooks() {
    // OpenCode loads plugins from `plugin/`, singular, and has no shell-command
    // hook: the plugin file is the only hook there is.
    assert_eq!(OpenCodeDiscovery.supported_hooks(), &["plugin"]);
}
