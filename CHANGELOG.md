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
