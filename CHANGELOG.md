# Changelog

All notable changes to viberails will be documented in this file.

## [Unreleased] - TBD

### Breaking Changes

- **Command restructuring for uninstall operations**:
  - `uninstall` command is now an alias for `uninstall-hooks` (previously it had "magic" behavior that also removed the binary when all hooks were uninstalled)
  - `uninstall-hooks` - New primary command to remove hooks only; keeps binary, config, and data intact
  - `uninstall-all` - New command for complete cleanup: removes all hooks, binary, config directory, data directory, and temporary files

- **Behavior change for `uninstall` / `uninstall-hooks`**:
  - Previously: When all hooks were uninstalled, the command would also delete the binary automatically
  - Now: The command only removes hooks from selected providers. Binary and configuration are always retained. Use `uninstall-all` for complete removal.

### Added

- GitHub Copilot CLI provider support (install, uninstall, list hooks)

- New `uninstall-all` CLI command and TUI menu option for complete cleanup
  - Confirmation prompt before proceeding (CLI: stdin y/N prompt, TUI: select prompt)
  - `--yes` / `-y` flag to skip confirmation for scripted/CI usage
  - Removes all hooks from all detected providers
  - Deletes the binary from `~/.local/bin/viberails`
  - Removes config directory (`~/.config/viberails/`)
  - Removes data directory (`~/.local/share/viberails/`) including debug logs and upgrade state
  - Cleans up upgrade lock files (`.viberails.upgrade.lock`)
  - Cleans up temporary upgrade binaries (`viberails_upgrade_*`, `.viberails_new_*`)
  - Symlink safety: refuses to follow symlinks to prevent attacks

- New `uninstall-hooks` CLI command (with `uninstall` as backward-compatible alias)
  - Removes hooks from selected providers
  - Explicitly keeps binary and config for future use
  - Displays "Binary retained for future use" message

- Comprehensive E2E tests for uninstall functionality:
  - `tests/e2e/uninstall_all.bats` - Tests for complete cleanup command
  - `tests/e2e/uninstall_hooks.bats` - Tests for hooks-only removal and backward compatibility

### Fixed

- **OpenCode provider now works.** It was registered and offered in the installer, but
  installing it had no effect: nothing ever invoked `viberails opencode-callback`.
  - Wrote hooks into an `opencode.json` key (`plugins`, an object of `{enabled, command}`)
    that OpenCode does not read. The real config key is `plugin`, an array, and OpenCode
    has no shell-command hook at all — plugins are JavaScript modules loaded from disk.
  - Resolved the config directory with `dirs::config_dir()`, which is
    `~/Library/Application Support` on macOS and `%APPDATA%` on Windows. OpenCode reads
    `$XDG_CONFIG_HOME/opencode`, falling back to `~/.config/opencode`, on every platform.
    OpenCode was therefore reported as "not detected" on macOS even when installed.
  - Now installs a generated plugin at `~/.config/opencode/plugin/<name>.js` and makes no
    changes to `opencode.json`. The plugin forwards tool calls to the callback binary over
    the same stdin/stdout protocol as every other provider, and denies a call by throwing,
    which surfaces the policy reason to the model.
  - `list` reports the binary path read back out of the installed plugin, so a plugin left
    pointing at a moved binary shows as stale rather than healthy.
  - `install`, `list` and `uninstall-hooks` all act only on a plugin viberails generated:
    any other file at that path is neither claimed, overwritten nor deleted, and any other
    plugin in the directory is left alone.
  - `OPENCODE_CONFIG` is no longer consulted. It names a config *file* that OpenCode merges
    and never moves plugin discovery, so installing beside it reported success while
    enforcing nothing. The plugin now always goes to the directory OpenCode globs.
  - The plugin no longer raises an uncaught exception inside OpenCode when the callback
    exits before reading stdin — which is what it does whenever the organization is
    unauthorized or the cloud cannot be reached. That EPIPE arrives as a stream `error`
    event, which a `try`/`catch` around the write cannot see.
  - Installing removes the obsolete `plugins` entry earlier versions wrote into
    `opencode.json`. Only that entry is touched, and never a config we cannot parse.

- **Primer D&R rules can now match OpenCode tool calls.** Every shipped rule compared
  `tool_name` against Claude Code's capitalized spelling (`Write`) and read arguments from
  snake_case keys (`tool_input/file_path`); OpenCode reports `write` and `tool_input/filePath`,
  so none of them could ever fire for it. The rules now match either spelling.
  - `vr-hook-config-tamper` watches the OpenCode plugin file rather than `opencode.json`,
    which viberails no longer writes: deleting the plugin is what disables enforcement.

### Security

- Added symlink attack protection in uninstall operations
  - `safe_remove_file()` and `safe_remove_dir_all()` functions check for symlinks before removal
  - Prevents malicious symlinks from tricking the uninstaller into deleting files outside viberails' control

### Changed

- TUI menu now shows "Remove Hooks" (shortcut `u`) for hook removal
- Replaced "Uninstall" menu option (shortcut `f`) with "Uninstall Everything" (shortcut `e`) for complete cleanup

### Related

- PR: #20

## [1.0.3] - 2026-02-06

- Less frequent upgrade polls (#24)

## [1.0.2] - 2026-02-05

- security: harden auto-upgrade mechanism (#15)

## [1.0.1] - 2026-02-04

- fix: restore configure CLI command (#22)
- fix: prevent hook process hang from upgrade FD leak and missing timeout (#21)

## [1.0.0] - 2026-02-03

- feat: auto-open team dashboard in browser after setup (#19)
- security: enforce secure permissions on config files and directories (#18)
- CI: Add cargo caching and streamline approval workflow (#14)
- Fix text wrapping in message component (#17)
- docs: add security note about curl|bash installer (#16)
- ux: show success message and exit after installation (#13)

## [0.1.x] - Earlier releases

Initial development releases with core functionality:
- Hook installation for Claude Code, Cursor, Gemini CLI, OpenAI Codex, OpenCode, and OpenClaw
- Team initialization and joining via OAuth
- Configuration management
- Auto-upgrade functionality
- Debug mode for troubleshooting
