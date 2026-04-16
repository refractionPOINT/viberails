# Viberails

**Enterprise-grade visibility and control for AI coding assistants**

Viberails gives your team complete oversight of AI coding tool activity across your organization. Know what your AI assistants are doing, catch risky operations before they happen, and maintain compliance—all without slowing down your developers.

## Why Viberails?

AI coding assistants like Claude Code, Cursor, and Copilot are transforming how developers work. But with great power comes great responsibility:

- **What tools are your AI assistants executing?** File operations, shell commands, API calls—do you know what's happening across your team?
- **Are sensitive files being accessed?** Configuration files, credentials, proprietary code—AI assistants can read and modify anything.
- **How do you maintain compliance?** Auditors want logs. Security teams want visibility. Viberails delivers both.

### The Value

| Without Viberails                | With Viberails                       |
| -------------------------------- | ------------------------------------ |
| No visibility into AI tool usage | Complete audit trail of every action |
| Risky operations go unnoticed    | Real-time authorization controls     |
| Compliance gaps                  | Full audit logs for security reviews |
| Each developer is an island      | Team-wide visibility and policies    |
| Hope nothing goes wrong          | Know exactly what's happening        |

## Features

- **Multi-Tool Support** - Works with the most popular AI coding assistants:
    - Claude Code
    - Cursor
    - Gemini CLI
    - GitHub Copilot CLI
    - OpenAI Codex CLI
    - OpenCode
    - OpenClaw

- **Zero Friction Setup** - Install in under 2 minutes with automatic tool detection

- **Team Collaboration** - One person sets up the team, everyone else joins with a single command

- **Privacy Controls** - Choose what gets sent to the cloud:
    - Full audit (tool calls + prompts)
    - Tool authorization only (no prompts)
    - Fully local operation

- **Fail-Safe Design** - Configurable fail-open/fail-closed behavior ensures developers aren't blocked

## Quick Start

### 1. Create Your Team

**macOS/Linux:**

```bash
bash <(curl -fsSL https://get.viberails.io/install.sh)
```

**Windows (PowerShell):**

```powershell
irm https://get.viberails.io/install.ps1 | iex
```

> **Security Note:** We get it - piping curl to bash requires trust. You can
> [review the install script](https://get.viberails.io/install.sh) before running,
> or download binaries directly:
> - [Linux x64](https://get.viberails.io/viberails-linux-x64) | [Linux ARM64](https://get.viberails.io/viberails-linux-arm64)
> - [macOS x64](https://get.viberails.io/viberails-macos-x64) | [macOS ARM64](https://get.viberails.io/viberails-macos-arm64)
> - [Windows x64](https://get.viberails.io/viberails-windows-x64.exe) | [Windows ARM64](https://get.viberails.io/viberails-windows-arm64.exe)
>
> Verify checksums via [release.json](https://get.viberails.io/release.json).

This downloads Viberails and launches the interactive setup. You'll be prompted to:
1. Select an OAuth provider (Google, Microsoft, or GitHub)
2. Enter your team name
3. Complete authentication in your browser

To use an existing LimaCharlie organization, run:

```bash
viberails init-team --existing-org <OID>
```

### 2. Install Hooks

**Interactive mode** (detects and lets you choose):

```bash
viberails install
```

**Non-interactive mode** (for scripts/automation):

```bash
# Install for specific tools
viberails install --providers claude-code

# Install for multiple tools
viberails install --providers claude-code,cursor,gemini-cli

# Install for all detected tools
viberails install --providers all
```

Viberails automatically detects which AI tools you have installed and lets you choose which ones to hook:

```
? Select AI coding tools to install hooks for:
  [x] Claude Code [detected]
  [x] Cursor [detected]
  [ ] Gemini CLI [not found]
  [ ] OpenAI Codex CLI [not found]
```

**Available provider IDs:** `claude-code`, `copilot`, `cursor`, `gemini-cli`, `codex`, `opencode`, `openclaw`

#### GitHub Copilot CLI: per-project hooks

Unlike the other supported tools — which install a single hook configuration in your home directory that applies everywhere — **GitHub Copilot CLI hooks are per-repository**. Copilot loads hooks from `.github/hooks/hooks.json` at the root of the current working directory (typically your git repository), so a single global install is not possible.

What this means in practice:

- **Run `viberails install` from inside each repository** where you want Copilot to be hooked. The hook file is written to `<repo>/.github/hooks/hooks.json`; if you're not inside a git repo, Viberails falls back to the current directory.
- **Commit the hooks file** if you want teammates and the Copilot cloud agent to pick up the same policy. The Copilot cloud agent reads `.github/hooks/` from the default branch.
- **`viberails list` and `viberails uninstall-hooks` are also directory-scoped** for Copilot — run them from the repo whose hooks you want to inspect or remove.
- **A fresh clone has no Viberails hook** until you either re-run `viberails install` in it or the file was committed to the repo.

All other providers (Claude Code, Cursor, Gemini CLI, Codex, OpenCode, OpenClaw) remain global and only need to be installed once per machine.

### 3. Add Your Team

After setup, you'll receive a command to share with your colleagues. They can join with a single command:

**Interactive mode** (choose tools during install):

```bash
# macOS/Linux
bash <(curl -fsSL https://get.viberails.io/install.sh) join-team <YOUR_TEAM_URL>

# Windows (PowerShell)
$u="<YOUR_TEAM_URL>"; irm https://get.viberails.io/join.ps1 | iex
```

**Non-interactive mode** (specify tools upfront):

```bash
# macOS/Linux - install for specific tools
bash <(curl -fsSL https://get.viberails.io/install.sh) join-team <YOUR_TEAM_URL> --providers claude-code

# macOS/Linux - install for all detected tools
bash <(curl -fsSL https://get.viberails.io/install.sh) join-team <YOUR_TEAM_URL> --providers all

# Windows (PowerShell) - install for specific tools
$u="<YOUR_TEAM_URL>"; $p="claude-code"; irm https://get.viberails.io/join.ps1 | iex

# Windows (PowerShell) - install for all detected tools
$u="<YOUR_TEAM_URL>"; $p="all"; irm https://get.viberails.io/join.ps1 | iex
```

That's it! Your team now has complete visibility into AI coding assistant activity.

## Configuration

### View Current Settings

```bash
viberails show-config
```

This displays your current configuration including:
- **Fail Open** - Whether tools are approved locally when cloud is unreachable (default: true)
- **Audit Tool Use** - Send tool calls to cloud for authorization (default: true)
- **Audit Prompts** - Send prompts/chat to cloud for audit logging (default: true)
- **Organization** - Your team name and webhook URL
- **Debug Mode** - Enable verbose logging for troubleshooting (default: disabled)
- **Auto Upgrade** - Automatic background updates (default: enabled)

Configuration can be modified by editing `~/.config/viberails/config.json`.

### Disabling Auto-Upgrade

Viberails automatically checks for and installs updates in the background. To disable this:

```bash
# Edit your config file
# On Linux/macOS: ~/.config/viberails/config.json
# On Windows: %APPDATA%\viberails\config.json
```

Set `auto_upgrade` to `false` in the `user` section:

```json
{
    "user": {
        "auto_upgrade": false
    }
}
```

When disabled, you can still manually upgrade with `viberails upgrade`.

## Commands Reference

| Command                     | Alias         | Description                                    |
| --------------------------- | ------------- | ---------------------------------------------- |
| `init-team`                 | `init`        | Create a new team via OAuth                    |
| `join-team <URL>`           | `join`        | Join an existing team                          |
| `install`                   |               | Install hooks for detected AI tools            |
| `install --providers <ids>` |               | Non-interactive install for specific providers |
| `uninstall-hooks`           | `uninstall`   | Remove hooks (keeps binary and config)         |
| `uninstall-all`             |               | Full cleanup: hooks, binary, config, data      |
| `list`                      | `ls`          | Show installed hooks                           |
| `show-config`               |               | Display current configuration                  |
| `upgrade`                   |               | Update to the latest version                   |
| `debug`                     |               | Enable/disable debug logging                   |
| `debug-clean`               | `clean-debug` | Remove accumulated debug logs                  |

Run `viberails --help` for detailed usage information.

## Troubleshooting

### Debug Mode

If hooks aren't working as expected, enable debug mode to capture detailed logs:

```bash
# Enable debug logging
viberails debug

# Use your AI coding tool - detailed logs will be captured
# ...

# View the logs
ls ~/.local/share/viberails/debug/
cat ~/.local/share/viberails/debug/debug-*.log

# Clean up logs when done (they accumulate over time)
viberails debug-clean

# Disable debug mode
viberails debug --disable
```

Debug logs include:
- Full payload data from AI tools
- Hook invocation details
- Cloud API request/response information
- Tool use vs prompt classification decisions

**Note:** Debug logs may contain sensitive information. Use only for troubleshooting and disable when done.

### Common Issues

**Hooks not triggering:**
1. Run `viberails list` to verify hooks are installed
2. Enable debug mode and check if logs are created when using the AI tool
3. Check the regular log at `~/.local/share/viberails/viberails.log`

**Events not reaching LimaCharlie:**
1. Enable debug mode to see cloud API responses
2. Verify your team URL with `viberails show-config`
3. Check if `audit_tool_use` and `audit_prompts` are enabled

## Architecture

### Backend: LimaCharlie

Viberails is powered by [LimaCharlie](https://limacharlie.io), a security infrastructure platform trusted by enterprises worldwide. This gives you:

- **Enterprise-grade reliability** - Built on battle-tested security infrastructure
- **Scalability** - From small teams to large organizations
- **Security** - Your data is handled with the same care as enterprise security telemetry
- **Extensibility** - Integrate with your existing security tools and workflows

### How It Works

```
┌─────────────────────────────────────────────────────────────┐
│ AI Coding Tool (Claude Code, Cursor, etc.)                  │
└────────────────────────┬────────────────────────────────────┘
                         │ Hook triggers on tool use
                         ▼
┌─────────────────────────────────────────────────────────────┐
│ Viberails Hook                                              │
│ • Captures tool call details                                │
│ • Sends to cloud for authorization                          │
│ • Returns allow/deny decision                               │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│ LimaCharlie Backend                                         │
│ • Stores audit logs                                         │
│ • Evaluates authorization policies                          │
│ • Provides team-wide visibility                             │
└─────────────────────────────────────────────────────────────┘
```

## Writing Custom D&R Rules

You can create custom Detection & Response (D&R) rules in the [web dashboard](https://app.viberails.io) to detect and block specific AI tool behaviors.

### Blocking Rules

Blocking rules use `webhook reject` to deny tool operations in real-time. For example:

**Detection:**
```yaml
target: webhook
op: and
rules:
  - op: is
    path: event/auth/tool_name
    value: Read
  - op: contains
    path: event/auth/tool_input/file_path
    value: .ssh/
```

**Response:**
```yaml
- action: report
  name: blocked ssh key access
- action: webhook reject
```

### Important: Routing Information Not Available in Blocking Rules

Blocking D&R rules are evaluated **before** the Sensor ID (SID) has been resolved. This means that the `routing` dictionary (which contains sensor metadata) is **not populated** at evaluation time.

The following **will not work** in blocking rules (rules with `target: webhook`):

- **Platform operators**: `is windows`, `is linux`, `is mac`, `is chrome`, `is net`, `is gcp`, `is carbon_black`, `is platform`, `is text`, `is json`
- **Architecture operators**: `is 32 bit`, `is 64 bit`, `is arm`
- **Tag operator**: `is tagged`
- **Any `path` referencing `routing/`**: e.g., `routing/sid`, `routing/tags`, `routing/hostname`, `routing/platform`

These operators rely on sensor identity and metadata that is resolved **after** the blocking decision has already been made. Rules using these operators will silently fail to match.

**What to do instead:** Write blocking rules that inspect the event payload directly (e.g., `event/auth/tool_name`, `event/auth/tool_input`). For rules that need routing/platform/tag information, use standard (non-blocking) D&R rules which evaluate after full event ingestion.

## Web Dashboard

Access your team's dashboard at [app.viberails.io](https://app.viberails.io):

- **Real-time Activity Feed** - Watch AI tool usage across your team as it happens
- **Policy Management** - Define rules for what tools can and cannot do
- **Analytics & Reporting** - Understand AI tool usage patterns
- **Alerts** - Get notified when sensitive operations occur
- **Compliance Reports** - Generate audit reports with one click

## Building from Source

```bash
# Clone the repository
git clone https://github.com/refractionPOINT/viberails.git
cd viberails

# Build
cargo build --release

# Run
./target/release/viberails --help
```

## Requirements

- Windows, macOS, or Linux
- One or more supported AI coding tools installed
- Internet connection for team features

The binary is installed to `~/.local/bin/viberails`. If this directory isn't in your PATH, add it:

```bash
# Add to ~/.bashrc or ~/.zshrc
export PATH="$HOME/.local/bin:$PATH"
```

## Contributing

We welcome contributions! Before submitting code:

```bash
cargo clippy -- -D warnings  # No warnings allowed
cargo test                   # All tests must pass
```

These checks are enforced by pre-commit hooks.

## Support

- **Issues**: [GitHub Issues](https://github.com/refractionPOINT/viberails/issues)
- **Documentation**: [docs.viberails.io](https://docs.viberails.io) (coming soon)
- **Community**: [Discord](https://discord.gg/viberails) (coming soon)

## License

Viberails is licensed under the [Apache License 2.0](LICENSE).

---

**Built with security in mind by [Refraction Point](https://refractionpoint.com), the team behind [LimaCharlie](https://limacharlie.io).**
