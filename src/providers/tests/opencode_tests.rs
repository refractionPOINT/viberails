#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use tempfile::TempDir;

use crate::common::PROJECT_NAME;
use crate::providers::LLmProviderTrait;
use crate::providers::opencode::{OPENCODE_HOOKS, OpenCode, OpenCodeDiscovery};

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
fn test_resolve_dir_prefers_opencode_config_parent() {
    // OPENCODE_CONFIG names a config *file*; the plugin dir is its parent.
    let resolved = OpenCodeDiscovery::resolve_dir(
        Some(PathBuf::from("/custom/oc/opencode.jsonc")),
        Some(PathBuf::from("/xdg")),
        Some(PathBuf::from("/home/eric")),
    );

    assert_eq!(resolved, Some(PathBuf::from("/custom/oc")));
}

#[test]
fn test_resolve_dir_falls_back_to_xdg_config_home() {
    let resolved = OpenCodeDiscovery::resolve_dir(
        None,
        Some(PathBuf::from("/xdg")),
        Some(PathBuf::from("/home/eric")),
    );

    assert_eq!(resolved, Some(PathBuf::from("/xdg/opencode")));
}

#[test]
fn test_resolve_dir_falls_back_to_dot_config_under_home() {
    // OpenCode uses ~/.config/opencode on every platform, including macOS and
    // Windows, so this must not follow the OS-native config location.
    let resolved = OpenCodeDiscovery::resolve_dir(None, None, Some(PathBuf::from("/home/eric")));

    assert_eq!(resolved, Some(PathBuf::from("/home/eric/.config/opencode")));
}

#[test]
fn test_resolve_dir_ignores_empty_env_values() {
    // An exported-but-empty variable must not win over the home fallback.
    let resolved = OpenCodeDiscovery::resolve_dir(
        Some(PathBuf::new()),
        Some(PathBuf::new()),
        Some(PathBuf::from("/home/eric")),
    );

    assert_eq!(resolved, Some(PathBuf::from("/home/eric/.config/opencode")));
}

#[test]
fn test_resolve_dir_without_home_is_none() {
    assert_eq!(OpenCodeDiscovery::resolve_dir(None, None, None), None);
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
