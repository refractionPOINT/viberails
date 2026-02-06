use std::time::Duration;

use anyhow::{Context, Result, bail};
use colored::Colorize;
use log::info;

use crate::{
    cloud::lc_api::{
        UserOrg, WebhookAdapter, create_installation_key, get_jwt_firebase, get_org_info,
        get_org_urls, get_user_orgs, org_available, org_create, signup_user,
    },
    common::PROJECT_NAME,
    config::{Config, LcOrg},
    default::get_embedded_default,
    oauth::{LoginArgs, OAuthProvider, authorize, primer_rules::create_dr_rules},
    tui::{
        ValidationResult,
        components::{Select, SelectItem},
        select_prompt, text_prompt,
    },
};

/// A selectable OAuth provider option for the interactive menu
struct ProviderOption {
    label: &'static str,
    provider: OAuthProvider,
}

/// Choice for team selection: create new or use existing
#[derive(Debug, Clone)]
enum TeamChoice {
    CreateNew,
    UseExisting { oid: String, name: String },
}

/// A selectable team option for the interactive menu
struct TeamOption {
    label: String,
    choice: TeamChoice,
}

const ORG_CREATE_TIMEOUT: Duration = Duration::from_mins(2);

const ORG_DEFAULT_LOCATION: &str = "auto";

fn query_user(prompt: &str) -> Result<Option<String>> {
    let validator = |s: &str| {
        if s.trim().is_empty() {
            ValidationResult::Invalid("Input cannot be empty".into())
        } else {
            ValidationResult::Valid
        }
    };
    let input = text_prompt(
        prompt,
        Some("Enter to submit, Esc to cancel"),
        Some(&validator),
    )
    .context("Failed to read user input")?;

    let Some(input) = input else {
        return Ok(None);
    };

    //
    // add a suffix for the org
    //
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let suffix = uuid.get(..8).unwrap_or(&uuid);
    let input = format!("{input}-{suffix}-vr");

    Ok(Some(input))
}

fn query_org_name(token: &str) -> Result<Option<String>> {
    loop {
        let Some(org_name) = query_user("Enter Team Name:")? else {
            return Ok(None);
        };

        let available = org_available(token, &org_name)?;

        if available {
            return Ok(Some(org_name));
        }

        println!("{}", format!("{org_name} isn't available").red());
    }
}

/// Returns all available OAuth provider options for the menu
fn get_provider_options() -> Vec<ProviderOption> {
    vec![
        ProviderOption {
            label: "Google (Recommended)",
            provider: OAuthProvider::Google,
        },
        ProviderOption {
            label: "Microsoft",
            provider: OAuthProvider::Microsoft,
        },
        ProviderOption {
            label: "GitHub",
            provider: OAuthProvider::GitHub,
        },
    ]
}

/// Prompt the user to select an OAuth provider
fn query_oauth_provider() -> Result<Option<OAuthProvider>> {
    let options = get_provider_options();
    let labels: Vec<&str> = options.iter().map(|o| o.label).collect();

    let selection_idx = select_prompt(
        "Select authentication provider:",
        labels,
        Some("↑↓ navigate, Enter select, Esc cancel"),
    )
    .context("Failed to read provider selection")?;

    let Some(idx) = selection_idx else {
        return Ok(None);
    };

    // Find the matching provider by index
    Ok(options.get(idx).map(|o| o.provider))
}

/// Prompt user to select a team (create new or use existing)
fn query_team_choice(orgs: Vec<UserOrg>) -> Result<Option<TeamChoice>> {
    let mut options: Vec<TeamOption> = vec![TeamOption {
        label: "Create a new team".to_string(),
        choice: TeamChoice::CreateNew,
    }];

    // Add existing orgs
    for org in orgs {
        let name = org.name.unwrap_or_else(|| org.oid.clone());
        options.push(TeamOption {
            label: name.clone(),
            choice: TeamChoice::UseExisting { oid: org.oid, name },
        });
    }

    let items: Vec<SelectItem<usize>> = options
        .iter()
        .enumerate()
        .map(|(idx, o)| SelectItem::new(idx, o.label.clone()))
        .collect();

    let selection_idx = Select::new("Select a team:", items)
        .with_starting_cursor(0)
        .with_help_message("↑↓ navigate, Enter select, Esc cancel")
        .prompt()
        .context("Failed to read team selection")?;

    let Some(idx) = selection_idx else {
        return Ok(None);
    };

    Ok(options.into_iter().nth(idx).map(|o| o.choice))
}

fn wait_for_org(oid: &str, token: &str) -> Result<String> {
    use std::time::Instant;

    let start = Instant::now();
    let retry_interval = Duration::from_secs(5);

    loop {
        match get_jwt_firebase(oid, token) {
            Ok(jwt) => return Ok(jwt),
            Err(e) => {
                if start.elapsed() >= ORG_CREATE_TIMEOUT {
                    return Err(e).context("Unable to get JWT TOKEN (timed out after 2 minutes)");
                }
                info!("Waiting for org to be ready... retrying in 5 seconds");
                std::thread::sleep(retry_interval);
            }
        }
    }
}

/// Creates a webhook adapter and returns the full webhook URL.
///
/// The webhook URL format is: <https://{hooks_domain}/{oid}/{adapter_name}/{secret}>
fn create_web_hook(oid: &str, jwt: &str, install_id: &str) -> Result<String> {
    //
    // Query org URLs to get the hooks domain
    //
    info!("Querying org URLs for oid={oid}");
    let urls = get_org_urls(oid).context("Failed to get org URLs")?;
    let hooks_domain = urls
        .hooks
        .context("Hook URL not available for this organization")?;
    info!("Hooks domain: {hooks_domain}");

    //
    // Create an installation key for the webhook adapter
    //
    info!("Creating installation key for webhook adapter");
    let key_desc = format!("{PROJECT_NAME} webhook adapter installation key");
    let installation_key = create_installation_key(jwt, oid, &key_desc)
        .context("Failed to create installation key")?;
    info!("Installation key created: {installation_key}");

    //
    // Generate a secret for the webhook URL
    //
    let secret = uuid::Uuid::new_v4().to_string();

    //
    // Create the webhook adapter in the cloud_sensor hive
    // Using install_id as sensor_seed_key so each installation gets a unique sensor
    //
    info!("Creating webhook adapter for oid={oid}");
    WebhookAdapter::builder()
        .token(jwt)
        .oid(oid)
        .name(PROJECT_NAME)
        .secret(&secret)
        .installation_key(&installation_key)
        .sensor_seed_key(install_id)
        .enabled(true)
        .build()
        .create()
        .context("Failed to create webhook adapter")?;
    info!("Webhook adapter created successfully");

    //
    // Construct the full webhook URL
    //
    let webhook_url = format!("https://{hooks_domain}/{oid}/{PROJECT_NAME}/{secret}");
    info!("Webhook URL: {webhook_url}");

    Ok(webhook_url)
}

////////////////////////////////////////////////////////////////////////////////
// Public
////////////////////////////////////////////////////////////////////////////////

pub fn login(args: &LoginArgs) -> Result<String> {
    // Ask user to select OAuth provider
    let Some(provider) = query_oauth_provider()? else {
        bail!("Invalid Oauth provider");
    };

    println!("\n{}", "Starting authentication...".cyan());
    let login = authorize(provider, args)?;
    println!("{} Authentication successful", "✓".green());

    //
    // Create LimaCharlie user profile if this is a new user.
    // This calls the same signUp Cloud Function that the web frontend uses.
    // The function is safe to call for existing users - it will return early.
    //
    if let Some(ref email) = login.email {
        println!("{} Setting up user profile...", "→".blue());
        info!("Creating user profile for {email}");
        signup_user(&login.id_token, email).context("Failed to create user profile")?;
    } else {
        info!("No email in OAuth response, skipping user profile creation");
    }

    //
    // Either use an existing org or create a new one
    //
    let (oid, org_name) = if let Some(ref existing_oid) = args.existing_org {
        // Use existing org - get a JWT for this specific org first
        // We need a JWT with permissions for this org to fetch its info
        println!("{} Looking up existing organization...", "→".blue());
        info!("Using existing org oid={existing_oid}");
        let jwt = wait_for_org(existing_oid, &login.id_token)?;
        let org_info =
            get_org_info(&jwt, existing_oid).context("Unable to get organization info")?;
        info!("Org name: {}", org_info.name);
        println!("{} Using team '{}'", "✓".green(), org_info.name);
        (existing_oid.clone(), org_info.name)
    } else {
        // Fetch user's existing orgs and show selection menu
        println!("{} Loading your teams...", "→".blue());
        let team_choice = match get_user_orgs(&login.id_token) {
            Ok(orgs) if !orgs.is_empty() => {
                let Some(choice) = query_team_choice(orgs)? else {
                    bail!("Team selection cancelled");
                };
                choice
            }
            Ok(_) => {
                // No existing orgs - go directly to create
                info!("User has no existing orgs");
                TeamChoice::CreateNew
            }
            Err(e) => {
                // API failed - fall back to create flow
                log::warn!("Failed to fetch user orgs: {e}");
                TeamChoice::CreateNew
            }
        };

        match team_choice {
            TeamChoice::CreateNew => {
                println!("{} Requesting access token...", "→".blue());
                info!("requesting a JWT token");
                let jwt = get_jwt_firebase("-", &login.id_token)
                    .context("Unable to get JWT from FB TOKEN")?;
                info!("received token");
                println!("{} Access token received", "✓".green());

                let Some(org_name) = query_org_name(&jwt)? else {
                    bail!("Organization name entry cancelled");
                };
                let location = ORG_DEFAULT_LOCATION;

                println!("{} Creating team '{}'...", "→".blue(), org_name);
                info!("Creating {org_name} org in {location}");
                let oid = org_create(&jwt, &org_name, location)
                    .context("Unable to create organization")?;
                info!("Org created oid={oid}");
                println!("{} Team created", "✓".green());
                (oid, org_name)
            }
            TeamChoice::UseExisting { oid, name } => {
                println!("{} Using existing team '{}'", "✓".green(), name);
                info!("Using existing org oid={oid}");
                (oid, name)
            }
        }
    };

    //
    // get the final token
    //
    println!("{} Finalizing setup...", "→".blue());
    info!("requesting a JWT token for {oid}");
    let jwt = wait_for_org(&oid, &login.id_token)?;
    info!("received token");

    let mut config = Config::load()?;
    println!("{} Creating webhook...", "→".blue());
    let url = create_web_hook(&oid, &jwt, &config.install_id)?;

    //
    // Create D&R rules for the team
    //
    println!("{} Creating detection rules...", "→".blue());
    create_dr_rules(&oid, &jwt)?;

    //
    // save the token to the config file
    //
    println!("{} Saving configuration...", "→".blue());
    let org = LcOrg {
        oid,
        name: org_name,
        url,
    };
    config.org = org;
    config.save()?;

    println!("{} Setup complete!", "✓".green());

    print_success_message(&config.org.name, &config.org.oid, &config.org.url);

    Ok(jwt)
}

/// Print the success message after team setup is complete
fn print_success_message(org_name: &str, oid: &str, webhook_url: &str) {
    let join_curl = get_embedded_default("join_team_command");
    let join_command = format!("{join_curl} {webhook_url}");
    let join_curl_windows = get_embedded_default("join_team_command_windows");
    let join_command_windows = join_curl_windows.replace("{URL}", webhook_url);
    let team_url = format!("https://app.viberails.io/viberails/teams/{oid}");

    // Calculate box width based on content
    // "  Team: " or "  View: " prefix is 8 chars, plus content, plus 2 for padding
    let team_line_len = 10_usize.saturating_add(org_name.len());
    let view_line_len = 10_usize.saturating_add(team_url.len());
    let box_width = team_line_len.max(view_line_len).max(60);

    // Pre-calculate padding for each line
    let team_padding = box_width.saturating_sub(10).saturating_sub(org_name.len());
    let view_padding = box_width.saturating_sub(10).saturating_sub(team_url.len());
    let inner_width = box_width.saturating_sub(2);

    println!();
    println!("  {}", "═".repeat(box_width).as_str().dimmed());
    println!();
    println!("  {} Setup complete!", "✓".green().bold());
    println!();
    println!(
        "  {}{}{}",
        "┌".yellow(),
        "─".repeat(inner_width).as_str().yellow(),
        "┐".yellow()
    );
    println!(
        "  {}  {}: {}{}{}",
        "│".yellow(),
        "Team".white().bold(),
        org_name.cyan().bold(),
        " ".repeat(team_padding),
        "│".yellow()
    );
    println!(
        "  {}  {}: {}{}{}",
        "│".yellow(),
        "View".white().bold(),
        team_url.yellow().bold().underline(),
        " ".repeat(view_padding),
        "│".yellow()
    );
    println!(
        "  {}{}{}",
        "└".yellow(),
        "─".repeat(inner_width).as_str().yellow(),
        "┘".yellow()
    );
    println!();
    println!("  {}", "─".repeat(box_width).as_str().dimmed());
    println!();
    println!("  {} Add other machines to this team:", "→".blue());
    println!();
    println!("  {}", "Linux/macOS:".dimmed());
    println!("    {}", join_command.cyan());
    println!();
    println!("  {}", "Windows (PowerShell):".dimmed());
    println!("    {}", join_command_windows.cyan());
    println!();
    println!("  {}", "─".repeat(box_width).as_str().dimmed());
    println!();
    println!(
        "  Powered by {} {}",
        "LimaCharlie".magenta().bold(),
        "https://limacharlie.io".dimmed()
    );
    println!();
    println!(
        "  {} {}",
        "Terms of Service:".dimmed(),
        "https://app.limacharlie.io/tos".dimmed()
    );
    println!();
    println!(
        "  {}",
        "TL;DR: Your data is your own—not sold, accessed, or monetized by".dimmed()
    );
    println!(
        "  {}",
        "LimaCharlie. Your team is created as a Community Edition Organization,".dimmed()
    );
    println!(
        "  {}",
        "completely free. Only limitation is over global throughput. Data is".dimmed()
    );
    println!(
        "  {}",
        "retained for 1 year unless you destroy the Organization.".dimmed()
    );
    println!();
    println!("  {}", "═".repeat(box_width).as_str().dimmed());
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_options_contains_all_providers() {
        let options = get_provider_options();

        // Verify we have options for all providers
        assert_eq!(options.len(), 3);

        let providers: Vec<_> = options.iter().map(|o| o.provider).collect();
        assert!(providers.contains(&OAuthProvider::Google));
        assert!(providers.contains(&OAuthProvider::Microsoft));
        assert!(providers.contains(&OAuthProvider::GitHub));
    }

    #[test]
    fn test_provider_options_google_is_first() {
        let options = get_provider_options();

        // Google should be the first option (default selection)
        assert_eq!(options[0].provider, OAuthProvider::Google);
    }

    #[test]
    fn test_provider_options_labels_are_unique() {
        let options = get_provider_options();
        let labels: Vec<_> = options.iter().map(|o| o.label).collect();

        // Verify all labels are unique
        let mut unique_labels = labels.clone();
        unique_labels.sort();
        unique_labels.dedup();
        assert_eq!(labels.len(), unique_labels.len());
    }

    #[test]
    fn test_provider_options_labels_not_empty() {
        let options = get_provider_options();

        // Verify no empty labels
        for option in &options {
            assert!(!option.label.is_empty());
        }
    }

    #[test]
    fn test_provider_lookup_finds_google() {
        let options = get_provider_options();
        let google_label = options
            .iter()
            .find(|o| o.provider == OAuthProvider::Google)
            .map(|o| o.label)
            .unwrap();

        // Simulate the lookup that query_oauth_provider does
        let found = options
            .into_iter()
            .find(|o| o.label == google_label)
            .map_or(OAuthProvider::Google, |o| o.provider);

        assert_eq!(found, OAuthProvider::Google);
    }

    #[test]
    fn test_provider_lookup_finds_microsoft() {
        let options = get_provider_options();
        let microsoft_label = options
            .iter()
            .find(|o| o.provider == OAuthProvider::Microsoft)
            .map(|o| o.label)
            .unwrap();

        // Simulate the lookup that query_oauth_provider does
        let found = options
            .into_iter()
            .find(|o| o.label == microsoft_label)
            .map_or(OAuthProvider::Google, |o| o.provider);

        assert_eq!(found, OAuthProvider::Microsoft);
    }

    #[test]
    fn test_provider_lookup_unknown_defaults_to_google() {
        let options = get_provider_options();

        // Simulate lookup with unknown label
        let found = options
            .into_iter()
            .find(|o| o.label == "Unknown Provider")
            .map_or(OAuthProvider::Google, |o| o.provider);

        assert_eq!(found, OAuthProvider::Google);
    }

    #[test]
    fn test_provider_lookup_finds_github() {
        let options = get_provider_options();
        let github_label = options
            .iter()
            .find(|o| o.provider == OAuthProvider::GitHub)
            .map(|o| o.label)
            .unwrap();

        // Simulate the lookup that query_oauth_provider does
        let found = options
            .into_iter()
            .find(|o| o.label == github_label)
            .map_or(OAuthProvider::Google, |o| o.provider);

        assert_eq!(found, OAuthProvider::GitHub);
    }
}
