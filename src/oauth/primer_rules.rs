//! Primer D&R (Detection & Response) rules for `VibeRails`.
//!
//! This module contains detection rules for security-relevant events like SSH key access,
//! hook configuration tampering, and binary modifications.

use anyhow::{Context, Result};
use log::info;
use serde_json::Value;

use crate::cloud::lc_api::DRRule;

// === Helper functions for rule generation ===

/// Generate Claude Code file path contains rules
fn cc_file_path_rules(paths: &[&str]) -> Vec<Value> {
    paths
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/tool_input/file_path",
                "value": p
            })
        })
        .collect()
}

/// Generate Claude Code command contains rules
fn cc_command_rules(patterns: &[&str]) -> Vec<Value> {
    patterns
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/tool_input/command",
                "value": p
            })
        })
        .collect()
}

/// Generate `OpenClaw` file path contains rules
fn oc_file_path_rules(paths: &[&str]) -> Vec<Value> {
    paths
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/params/path",
                "value": p
            })
        })
        .collect()
}

/// Generate `OpenClaw` command contains rules
fn oc_command_rules(patterns: &[&str]) -> Vec<Value> {
    patterns
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/params/command",
                "value": p
            })
        })
        .collect()
}

/// Generate a Claude Code tool detection rule (`tool_name` + condition)
#[allow(clippy::needless_pass_by_value)]
fn cc_tool_rule(tool: &str, condition_rules: Vec<Value>) -> Value {
    serde_json::json!({
        "op": "and",
        "rules": [
            { "op": "is", "path": "event/auth/tool_name", "value": tool },
            { "op": "or", "rules": condition_rules }
        ]
    })
}

/// Generate an `OpenClaw` tool detection rule (`toolName` + condition)
#[allow(clippy::needless_pass_by_value)]
fn oc_tool_rule(tool: &str, condition_rules: Vec<Value>) -> Value {
    serde_json::json!({
        "op": "and",
        "rules": [
            { "op": "is", "path": "event/auth/toolName", "value": tool },
            { "op": "or", "rules": condition_rules }
        ]
    })
}

/// Generate Claude Code file extension regex rules (for Write tool)
fn cc_file_extension_rules(extensions: &[&str]) -> Vec<Value> {
    extensions
        .iter()
        .map(|ext| {
            let escaped_ext = ext.replace('.', r"\.");
            serde_json::json!({
                "op": "matches",
                "path": "event/auth/tool_input/file_path",
                "re": format!(r"{}$", escaped_ext)
            })
        })
        .collect()
}

/// Generate `OpenClaw` file extension regex rules (for write tool)
fn oc_file_extension_rules(extensions: &[&str]) -> Vec<Value> {
    extensions
        .iter()
        .map(|ext| {
            let escaped_ext = ext.replace('.', r"\.");
            serde_json::json!({
                "op": "matches",
                "path": "event/auth/params/path",
                "re": format!(r"{}$", escaped_ext)
            })
        })
        .collect()
}

/// Creates all D&R rules for the team.
///
/// This creates detection rules for security-relevant events like SSH key access,
/// hook configuration tampering, and binary modifications.
/// Rules are created in parallel to improve performance.
pub fn create_dr_rules(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating D&R rules for oid={oid}");

    // Define all rule creation functions
    let rule_creators: Vec<fn(&str, &str) -> Result<()>> = vec![
        create_ssh_access_rule,
        create_hook_config_tamper_rule,
        create_binary_tamper_rule,
        create_persistence_modification_rule,
        create_cloud_creds_access_rule,
        create_email_sending_rule,
        create_macos_sensitive_data_rule,
        create_destructive_delete_rule,
        create_suspicious_tlds_rule,
        create_file_encryption_rule,
    ];

    // Create all rules in parallel using scoped threads
    let results: Vec<Result<()>> = std::thread::scope(|s| {
        let handles: Vec<_> = rule_creators
            .into_iter()
            .map(|create_fn| s.spawn(move || create_fn(oid, jwt)))
            .collect();

        // Collect all results, converting thread panics to errors
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| anyhow::anyhow!("Rule creation thread panicked"))?
            })
            .collect()
    });

    // Check for any errors and return the first one
    for result in results {
        result?;
    }

    info!("D&R rules created successfully");
    Ok(())
}

/// Rule: Detect SSH key access
fn create_ssh_access_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating SSH key access detection rule");
    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // === Claude Code detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "Read" },
                    { "op": "contains", "path": "event/auth/tool_input/file_path", "value": ".ssh" }
                ]
            },
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "Bash" },
                    { "op": "contains", "path": "event/auth/tool_input/command", "value": ".ssh" }
                ]
            },
            { "op": "contains", "path": "event/auth/cwd", "value": ".ssh" },
            // === OpenClaw detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "read" },
                    { "op": "contains", "path": "event/auth/params/path", "value": ".ssh" }
                ]
            },
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "exec" },
                    { "op": "contains", "path": "event/auth/params/command", "value": ".ssh" }
                ]
            }
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "reading ssh keys"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-ssh-key-access")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create SSH key access D&R rule")
}

/// Rule: Detect hook configuration file modifications
/// Monitors: Claude, Copilot, Cursor, Gemini, Codex, `OpenCode`, `OpenClaw` config files
fn create_hook_config_tamper_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating hook config tampering detection rule");
    // Hook config file patterns to monitor
    // NOTE: Keep in sync with providers in src/providers/*.rs
    let config_files = [
        ".claude/settings.json",          // Claude Code
        ".cursor/hooks.json",             // Cursor
        ".gemini/settings.json",          // Gemini CLI
        ".github/hooks/hooks.json",       // GitHub Copilot CLI (per-project)
        ".codex/config.toml",             // OpenAI Codex CLI
        ".config/opencode/opencode.json", // OpenCode
        ".openclaw/openclaw.json",        // OpenClaw
    ];

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // Claude Code: Write/Edit/Bash
            cc_tool_rule("Write", cc_file_path_rules(&config_files)),
            cc_tool_rule("Edit", cc_file_path_rules(&config_files)),
            cc_tool_rule("Bash", cc_command_rules(&config_files)),
            // OpenClaw: write/exec
            oc_tool_rule("write", oc_file_path_rules(&config_files)),
            oc_tool_rule("exec", oc_command_rules(&config_files)),
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "hook config modification"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-hook-config-tamper")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create hook config tampering D&R rule")
}

/// Rule: Detect viberails binary modifications
/// Monitors: ~/.local/bin/viberails
fn create_binary_tamper_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating viberails binary tampering detection rule");
    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // === Claude Code detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "Write" },
                    { "op": "contains", "path": "event/auth/tool_input/file_path", "value": ".local/bin/viberails" }
                ]
            },
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "Bash" },
                    { "op": "contains", "path": "event/auth/tool_input/command", "value": ".local/bin/viberails" }
                ]
            },
            // === OpenClaw detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "write" },
                    { "op": "contains", "path": "event/auth/params/path", "value": ".local/bin/viberails" }
                ]
            },
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "exec" },
                    { "op": "contains", "path": "event/auth/params/command", "value": ".local/bin/viberails" }
                ]
            }
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "viberails binary modification"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-binary-tamper")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create binary tampering D&R rule")
}

/// Rule: Detect persistence mechanism installations
/// Monitors: cron, systemd, init.d, `LaunchAgents`, scheduled tasks, registry Run keys
fn create_persistence_modification_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating persistence modification detection rule");

    // File paths that indicate persistence (for Write/Edit tools)
    let persistence_paths = [
        // Linux
        "/etc/cron",
        "/etc/systemd",
        "/etc/init.d",
        "/etc/rc.local",
        "/etc/profile",
        ".bashrc",
        ".profile",
        // macOS
        "LaunchAgents",
        "LaunchDaemons",
        "/Library/StartupItems",
    ];

    // Bash command patterns for persistence
    let persistence_commands = [
        // Linux
        "crontab",
        "/etc/cron",
        "/etc/systemd",
        "/etc/init.d",
        "/etc/rc.local",
        // macOS
        "LaunchAgents",
        "LaunchDaemons",
        // Windows
        "schtasks",
        "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        "sc create",
        "New-ScheduledTask",
        "Register-ScheduledTask",
    ];

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // Claude Code: Write/Edit/Bash
            cc_tool_rule("Write", cc_file_path_rules(&persistence_paths)),
            cc_tool_rule("Edit", cc_file_path_rules(&persistence_paths)),
            cc_tool_rule("Bash", cc_command_rules(&persistence_commands)),
            // OpenClaw: write/exec
            oc_tool_rule("write", oc_file_path_rules(&persistence_paths)),
            oc_tool_rule("exec", oc_command_rules(&persistence_commands)),
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "persistence mechanism modification"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-persistence-modification")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create persistence modification D&R rule")
}

/// Rule: Detect cloud credentials access
/// Monitors: AWS, GCP, Azure, Kubernetes, Docker, Terraform credentials
fn create_cloud_creds_access_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating cloud credentials access detection rule");

    let cred_paths = [
        // AWS
        ".aws/credentials",
        ".aws/config",
        // GCP
        ".config/gcloud",
        "application_default_credentials.json",
        // Azure
        ".azure",
        // Kubernetes
        ".kube/config",
        // Docker
        ".docker/config.json",
        // Terraform
        "terraform.tfstate",
    ];

    // Command patterns include cred paths + env vars
    let cred_cmd_patterns = [
        ".aws/credentials",
        ".aws/config",
        ".config/gcloud",
        "application_default_credentials.json",
        ".azure",
        ".kube/config",
        ".docker/config.json",
        "terraform.tfstate",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
    ];

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // Claude Code: Read/Bash
            cc_tool_rule("Read", cc_file_path_rules(&cred_paths)),
            cc_tool_rule("Bash", cc_command_rules(&cred_cmd_patterns)),
            // OpenClaw: read/exec
            oc_tool_rule("read", oc_file_path_rules(&cred_paths)),
            oc_tool_rule("exec", oc_command_rules(&cred_cmd_patterns)),
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "cloud credentials access"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-cloud-creds-access")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create cloud credentials access D&R rule")
}

/// Rule: Detect email sending via local tools
/// Monitors: sendmail, mail, postfix, `PowerShell`, Python SMTP, email APIs
fn create_email_sending_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating email sending detection rule");

    let email_patterns = [
        // Unix mail tools
        "sendmail",
        "mailx",
        " mail ",
        "mutt",
        // MTAs
        "postfix",
        "exim",
        "postqueue",
        // PowerShell
        "Send-MailMessage",
        // Python
        "smtplib",
        "smtp.sendmail",
        // Email APIs
        "api.sendgrid.com",
        "api.mailgun.net",
    ];

    // Claude Code command rules
    let command_rules: Vec<_> = email_patterns
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/tool_input/command",
                "value": p
            })
        })
        .collect();

    // OpenClaw command rules
    let oc_command_rules: Vec<_> = email_patterns
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/params/command",
                "value": p
            })
        })
        .collect();

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // === Claude Code detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "Bash" },
                    { "op": "or", "rules": command_rules }
                ]
            },
            // === OpenClaw detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "exec" },
                    { "op": "or", "rules": oc_command_rules }
                ]
            }
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "email sending detected"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-email-sending")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create email sending D&R rule")
}

/// Rule: Detect macOS sensitive data access
/// Monitors: Keychain, browser data, Notes, Messages
fn create_macos_sensitive_data_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating macOS sensitive data access detection rule");

    let sensitive_paths = [
        // Keychain
        "Library/Keychains",
        "login.keychain",
        // Chrome
        "Application Support/Google/Chrome/Default/Cookies",
        "Application Support/Google/Chrome/Default/Login Data",
        // Safari
        "Library/Safari/Cookies",
        "Cookies.binarycookies",
        // Firefox
        "cookies.sqlite",
        "logins.json",
        "key4.db",
        // Notes
        "apple.Notes",
        "NoteStore.sqlite",
        // Messages
        "Library/Messages/chat.db",
    ];

    // Command patterns include sensitive paths + security commands
    let cmd_patterns = [
        "Library/Keychains",
        "login.keychain",
        "Application Support/Google/Chrome/Default/Cookies",
        "Application Support/Google/Chrome/Default/Login Data",
        "Library/Safari/Cookies",
        "Cookies.binarycookies",
        "cookies.sqlite",
        "logins.json",
        "key4.db",
        "apple.Notes",
        "NoteStore.sqlite",
        "Library/Messages/chat.db",
        "security find-",
        "security dump-keychain",
    ];

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // Claude Code: Read/Bash
            cc_tool_rule("Read", cc_file_path_rules(&sensitive_paths)),
            cc_tool_rule("Bash", cc_command_rules(&cmd_patterns)),
            // OpenClaw: read/exec
            oc_tool_rule("read", oc_file_path_rules(&sensitive_paths)),
            oc_tool_rule("exec", oc_command_rules(&cmd_patterns)),
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "macOS sensitive data access"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-macos-sensitive-data")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create macOS sensitive data D&R rule")
}

/// Rule: Detect dangerous destructive delete operations
/// Monitors: rm -rf /, format C:, dd to devices, mkfs, shred, wipefs
fn create_destructive_delete_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating destructive delete detection rule");

    let destructive_patterns = [
        // Unix dangerous rm
        "rm -rf /",
        "rm -rf /*",
        "rm --no-preserve-root",
        // Windows
        "del /s /q C:\\",
        "rd /s /q C:\\",
        "format C:",
        // Disk operations
        "dd if=/dev/zero of=/dev/sd",
        "dd if=/dev/zero of=/dev/nvme",
        "dd if=/dev/urandom of=/dev/sd",
        "dd if=/dev/urandom of=/dev/nvme",
        // Filesystem tools
        "mkfs.",
        "shred ",
        "wipefs",
    ];

    // Claude Code command rules
    let command_rules: Vec<_> = destructive_patterns
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/tool_input/command",
                "value": p
            })
        })
        .collect();

    // OpenClaw command rules
    let oc_command_rules: Vec<_> = destructive_patterns
        .iter()
        .map(|p| {
            serde_json::json!({
                "op": "contains",
                "path": "event/auth/params/command",
                "value": p
            })
        })
        .collect();

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // === Claude Code detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "Bash" },
                    { "op": "or", "rules": command_rules }
                ]
            },
            // === OpenClaw detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "exec" },
                    { "op": "or", "rules": oc_command_rules }
                ]
            }
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "destructive delete operation"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-destructive-delete")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create destructive delete D&R rule")
}

/// Rule: Detect access to suspicious TLD domains
/// Monitors: `WebFetch` URLs and curl/wget commands to suspicious TLDs
fn create_suspicious_tlds_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating suspicious TLDs detection rule");

    let suspicious_tlds = [
        ".ru", ".cn", ".tk", ".ml", ".ga", ".cf", ".top", ".xyz", ".pw",
        ".cc", ".ws", ".gq", ".work", ".click", ".link", ".monster", ".icu", ".buzz",
    ];

    // Claude Code URL rules for WebFetch
    let url_rules: Vec<_> = suspicious_tlds
        .iter()
        .map(|tld| {
            // Escape the dot in the TLD for regex (e.g., ".ru" -> r"\.ru")
            let escaped_tld = tld.replace('.', r"\.");
            serde_json::json!({
                "op": "matches",
                "path": "event/auth/tool_input/url",
                "re": format!(r"https?://[^/]*{}(/|$)", escaped_tld)
            })
        })
        .collect();

    // Claude Code command rules for Bash
    let command_rules: Vec<_> = suspicious_tlds
        .iter()
        .map(|tld| {
            let escaped_tld = tld.replace('.', r"\.");
            serde_json::json!({
                "op": "matches",
                "path": "event/auth/tool_input/command",
                "re": format!(r"(curl|wget).*https?://[^\s]*{}", escaped_tld)
            })
        })
        .collect();

    // OpenClaw query rules for web_search (query may contain URLs)
    let oc_query_rules: Vec<_> = suspicious_tlds
        .iter()
        .map(|tld| {
            let escaped_tld = tld.replace('.', r"\.");
            serde_json::json!({
                "op": "matches",
                "path": "event/auth/params/query",
                "re": format!(r"https?://[^\s]*{}", escaped_tld)
            })
        })
        .collect();

    // OpenClaw command rules for exec
    let oc_command_rules: Vec<_> = suspicious_tlds
        .iter()
        .map(|tld| {
            let escaped_tld = tld.replace('.', r"\.");
            serde_json::json!({
                "op": "matches",
                "path": "event/auth/params/command",
                "re": format!(r"(curl|wget).*https?://[^\s]*{}", escaped_tld)
            })
        })
        .collect();

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // === Claude Code detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "WebFetch" },
                    { "op": "or", "rules": url_rules }
                ]
            },
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/tool_name", "value": "Bash" },
                    { "op": "or", "rules": command_rules }
                ]
            },
            // === OpenClaw detection ===
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "web_search" },
                    { "op": "or", "rules": oc_query_rules }
                ]
            },
            {
                "op": "and",
                "rules": [
                    { "op": "is", "path": "event/auth/toolName", "value": "exec" },
                    { "op": "or", "rules": oc_command_rules }
                ]
            }
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "suspicious TLD access"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-suspicious-tlds")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create suspicious TLDs D&R rule")
}

/// Rule: Detect file encryption activity
/// Monitors: openssl, gpg, 7z/zip encryption, Python crypto, suspicious extensions
fn create_file_encryption_rule(oid: &str, jwt: &str) -> Result<()> {
    info!("Creating file encryption detection rule");

    let encryption_commands = [
        // OpenSSL
        "openssl enc",
        "openssl aes",
        // GPG
        "gpg -e",
        "gpg --encrypt",
        "gpg -c",
        "gpg --symmetric",
        // Archive encryption
        "7z a -p",
        "zip -e",
        "zip --encrypt",
        // Python crypto
        "cryptography.fernet",
        "Fernet(",
        "from Crypto",
        "AES.new",
        // PowerShell
        "ConvertTo-SecureString",
    ];

    let encrypted_extensions = [".encrypted", ".locked", ".crypted", ".enc"];

    let detect = serde_json::json!({
        "op": "or",
        "rules": [
            // Claude Code: Bash/Write
            cc_tool_rule("Bash", cc_command_rules(&encryption_commands)),
            cc_tool_rule("Write", cc_file_extension_rules(&encrypted_extensions)),
            // OpenClaw: exec/write
            oc_tool_rule("exec", oc_command_rules(&encryption_commands)),
            oc_tool_rule("write", oc_file_extension_rules(&encrypted_extensions)),
        ],
        "target": "webhook"
    });

    let respond = vec![serde_json::json!({
        "action": "report",
        "name": "file encryption activity"
    })];

    DRRule::builder()
        .token(jwt)
        .oid(oid)
        .name("vr-file-encryption")
        .detect(detect)
        .respond(respond)
        .build()
        .create()
        .context("Failed to create file encryption D&R rule")
}
