//! OAuth authorization flow using Firebase authentication.
//!
//! This implementation uses Firebase's createAuthUri approach which eliminates
//! the need to manage OAuth provider credentials directly. Instead of handling
//! Google OAuth ourselves, we let Firebase manage the OAuth flow.

use anyhow::{Context, Result, anyhow};
use log::{error, info, warn};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use tiny_http::{Response, Server};
use url::Url;

use crate::common::user_agent;
use crate::default::get_embedded_default;
use crate::tui::{ValidationResult, text_prompt};

#[derive(Embed)]
#[folder = "resources/oauth/"]
struct OAuthAssets;

/// Firebase Auth API endpoints
const CREATE_AUTH_URI: &str = "https://identitytoolkit.googleapis.com/v1/accounts:createAuthUri";
const SIGN_IN_WITH_IDP: &str = "https://identitytoolkit.googleapis.com/v1/accounts:signInWithIdp";
const MFA_FINALIZE: &str = "https://identitytoolkit.googleapis.com/v2/accounts/mfaSignIn:finalize";

/// GitHub Device Flow endpoints
const GITHUB_DEVICE_CODE: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN: &str = "https://github.com/login/oauth/access_token";

/// GitHub Device Flow polling interval (seconds)
const GITHUB_POLL_INTERVAL: u64 = 5;

/// OAuth callback timeout in seconds
const OAUTH_CALLBACK_TIMEOUT: u64 = 300;

/// Preferred ports for OAuth callback server
const PREFERRED_PORTS: &[u16] = &[8085, 8086, 8087, 8088, 8089];

/// OAuth provider identifiers
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum, PartialEq, Eq)]
pub enum OAuthProvider {
    #[default]
    Google,
    Microsoft,
    GitHub,
}

impl OAuthProvider {
    fn as_firebase_id(self) -> &'static str {
        match self {
            Self::Google => "google.com",
            Self::Microsoft => "microsoft.com",
            Self::GitHub => "github.com",
        }
    }
}

/// Result of a successful OAuth authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub id_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Request payload for Firebase createAuthUri
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateAuthUriRequest {
    provider_id: String,
    continue_uri: String,
    auth_flow_type: String,
    oauth_scope: String,
}

/// Response from Firebase createAuthUri
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAuthUriResponse {
    session_id: String,
    auth_uri: String,
}

/// Request payload for Firebase signInWithIdp
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInWithIdpRequest {
    request_uri: String,
    post_body: String,
    session_id: String,
    return_secure_token: bool,
    return_idp_credential: bool,
}

/// Response from Firebase signInWithIdp (success case - no MFA)
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignInWithIdpResponse {
    id_token: String,
    refresh_token: String,
    expires_in: String,
    local_id: Option<String>,
    email: Option<String>,
}

/// Response from Firebase signInWithIdp when MFA is required
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignInWithIdpMfaResponse {
    mfa_pending_credential: String,
    mfa_info: Vec<MfaFactorInfo>,
    local_id: Option<String>,
    email: Option<String>,
}

/// MFA factor information
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct MfaFactorInfo {
    mfa_enrollment_id: String,
    display_name: Option<String>,
    #[serde(default)]
    totp_info: Option<serde_json::Value>,
    #[serde(default)]
    #[allow(dead_code)] // Parsed from API response, kept for future SMS MFA support
    phone_info: Option<String>,
}

/// Request payload for MFA finalize
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MfaFinalizeRequest {
    mfa_pending_credential: String,
    mfa_enrollment_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    totp_verification_info: Option<TotpVerificationInfo>,
}

/// TOTP verification info for MFA
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TotpVerificationInfo {
    verification_code: String,
}

/// Response from MFA finalize
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MfaFinalizeResponse {
    id_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: Option<String>,
}

/// GitHub Device Flow: Response with device and user codes
#[derive(Deserialize)]
struct GitHubDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

/// GitHub Device Flow: Access token response
#[derive(Deserialize)]
struct GitHubAccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Firebase signInWithIdp request for direct OAuth credential exchange.
///
/// This is separate from `SignInWithIdpRequest` because credential-based auth
/// (exchanging an access token directly) doesn't require `session_id`, which
/// is only needed for the callback-based flow.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInWithCredentialRequest {
    post_body: String,
    request_uri: String,
    return_secure_token: bool,
    return_idp_credential: bool,
}

/// Callback result from the OAuth server
enum CallbackResult {
    /// Authentication completed successfully (tokens ready)
    Success(OAuthTokens),
    /// Authentication failed with an error message
    Error(String),
    /// Authentication timed out
    Timeout,
}

/// MFA session state for browser-based verification
struct MfaSession {
    mfa_pending_credential: String,
    mfa_enrollment_id: String,
    factor_display_name: String,
    provider: OAuthProvider,
    local_id: Option<String>,
    email: Option<String>,
}

#[derive(clap::Args)]
pub struct LoginArgs {
    /// Print the URL instead of opening a browser
    #[arg(long)]
    pub no_browser: bool,

    /// Use an existing organization instead of creating a new one
    #[arg(long, value_name = "OID")]
    pub existing_org: Option<String>,
}

/// Perform OAuth authorization flow.
///
/// This function:
/// 1. For GitHub: Uses Device Flow (no callback server needed)
/// 2. For others with browser: Starts a local HTTP server to receive the OAuth callback
/// 3. For others without browser: Uses manual callback URL entry
/// 4. Requests an auth URI from Firebase (or device code from GitHub)
/// 5. Opens the browser (or prints the URL) for user authentication
/// 6. Waits for the callback/polling completion
/// 7. Exchanges the code/token for Firebase tokens
///
/// # Arguments
/// * `provider` - The OAuth provider to use for authentication
/// * `args` - Additional configuration for the authorization flow
///
/// # Returns
/// * `Ok(OAuthTokens)` - The OAuth tokens on success
/// * `Err` - An error if authorization fails
pub fn authorize(provider: OAuthProvider, args: &LoginArgs) -> Result<OAuthTokens> {
    // GitHub uses Device Flow (no callback server needed)
    if provider == OAuthProvider::GitHub {
        return authorize_github_device_flow(args);
    }

    // Check if we should use manual callback mode (no browser available)
    let use_manual_callback = args.no_browser || !is_browser_available();

    if use_manual_callback {
        return authorize_manual_callback(provider);
    }

    // Find a free port and start the callback server
    let port = find_free_port()?;
    let redirect_uri = format!("http://localhost:{port}/callback");

    println!("OAuth callback server started on port {port}");
    println!("Using OAuth provider: {}", provider.as_firebase_id());

    // Get auth URI from Firebase first (we need session_id for the callback server)
    let (session_id, auth_uri) = create_auth_uri(provider, &redirect_uri)?;

    // Start the callback server in a separate thread
    let (tx, rx): (Sender<CallbackResult>, Receiver<CallbackResult>) = mpsc::channel();
    let server = Server::http(format!("127.0.0.1:{port}"))
        .map_err(|e| anyhow!("Failed to start OAuth callback server: {e}"))?;

    let redirect_uri_clone = redirect_uri.clone();
    let server_handle = thread::spawn(move || {
        run_callback_server(&server, &tx, &redirect_uri_clone, provider, &session_id);
    });

    // Open browser
    println!("Opening browser for authentication...");
    if open_browser(&auth_uri).is_err() {
        // Browser failed to open - fall back to manual callback mode
        println!("\nCould not open browser. Switching to manual mode...\n");
        drop(server_handle);
        return authorize_manual_callback(provider);
    }

    println!("Waiting for authentication...");

    // Wait for callback - the server now handles the full flow including MFA
    let callback_result = rx
        .recv_timeout(Duration::from_secs(OAUTH_CALLBACK_TIMEOUT))
        .unwrap_or(CallbackResult::Timeout);

    // Clean up the server thread (it will exit when the server is dropped)
    drop(server_handle);

    match callback_result {
        CallbackResult::Success(tokens) => Ok(tokens),
        CallbackResult::Error(error) => Err(anyhow!("Authentication failed: {error}")),
        CallbackResult::Timeout => Err(anyhow!("Authentication timeout")),
    }
}

/// Check if a browser is likely available on this system.
///
/// Returns false for headless systems (no DISPLAY on Linux, SSH sessions, etc.)
#[must_use]
pub fn is_browser_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Check for DISPLAY environment variable (X11)
        // or WAYLAND_DISPLAY (Wayland)
        let has_display =
            std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok();

        // Also check if we're in an SSH session without X forwarding
        let in_ssh = std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok();
        let has_x_forwarding = std::env::var("DISPLAY").is_ok();

        if in_ssh && !has_x_forwarding {
            return false;
        }

        has_display
    }

    #[cfg(target_os = "macos")]
    {
        // macOS almost always has a browser unless we're in a pure SSH session
        // Check if we're in an SSH session
        let in_ssh = std::env::var("SSH_CLIENT").is_ok() || std::env::var("SSH_TTY").is_ok();
        !in_ssh
    }

    #[cfg(target_os = "windows")]
    {
        // Windows typically always has a browser
        true
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // Unknown platform - assume no browser
        false
    }
}

/// Perform OAuth authorization with manual callback URL entry.
///
/// This flow is used when no browser is available:
/// 1. Print the auth URL for the user to visit on any device
/// 2. User completes authorization in their browser
/// 3. Browser redirects to localhost (which fails)
/// 4. User copies the full URL from browser and pastes it here
/// 5. We extract the code and exchange for tokens
fn authorize_manual_callback(provider: OAuthProvider) -> Result<OAuthTokens> {
    // Use a placeholder redirect URI - we won't actually receive callbacks
    let redirect_uri = "http://localhost:8085/callback";

    println!("Using OAuth provider: {}", provider.as_firebase_id());
    println!("No browser detected. Using manual authentication flow.\n");

    // Get auth URI from Firebase
    let (session_id, auth_uri) = create_auth_uri(provider, redirect_uri)?;

    // Print instructions
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                   Manual Authentication                      ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ 1. Open this URL in a browser (on any device):               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("  {auth_uri}");
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║ 2. Complete the sign-in process                              ║");
    println!("║ 3. After signing in, your browser will show an error page    ║");
    println!("║    (\"localhost refused to connect\" or similar)               ║");
    println!("║ 4. Copy the FULL URL from your browser's address bar         ║");
    println!("║    It will look like: http://localhost:8085/callback?code=...║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Prompt for the callback URL
    let validator = |input: &str| {
        if input.contains("code=") || input.contains("error=") {
            ValidationResult::Valid
        } else {
            ValidationResult::Invalid("URL must contain 'code=' or 'error=' parameter".into())
        }
    };
    let callback_url = text_prompt(
        "Paste the callback URL here:",
        Some("Enter to submit, Esc to cancel"),
        Some(&validator),
    )
    .map_err(|e| anyhow!("Input error: {e}"))?;

    let Some(callback_url) = callback_url else {
        return Err(anyhow!("Input cancelled"));
    };

    // Parse the callback URL
    let parsed_url = if callback_url.starts_with("http") {
        Url::parse(&callback_url)
    } else {
        // Handle if user just pasted the path
        Url::parse(&format!("http://localhost{callback_url}"))
    }
    .context("Invalid callback URL format")?;

    let params: HashMap<_, _> = parsed_url.query_pairs().collect();

    // Check for error
    if let Some(error) = params.get("error") {
        let error_desc = params
            .get("error_description")
            .map_or_else(|| error.to_string(), ToString::to_string);
        return Err(anyhow!("Authentication failed: {error_desc}"));
    }

    // Extract authorization code
    let query_string = parsed_url
        .query()
        .ok_or_else(|| anyhow!("No query parameters in callback URL"))?;

    if !params.contains_key("code") {
        return Err(anyhow!(
            "No authorization code in callback URL. Please make sure you copied the complete URL."
        ));
    }

    println!("\nExchanging authorization code for tokens...");

    // Exchange the code for tokens
    match exchange_code_for_tokens(redirect_uri, query_string, &session_id, provider)? {
        ExchangeResult::Success(tokens) => {
            println!("Authentication successful!");
            Ok(tokens)
        }
        ExchangeResult::MfaRequired(session) => {
            // Handle MFA in CLI
            println!();
            handle_mfa_in_cli(
                &session.mfa_pending_credential,
                &session.mfa_enrollment_id,
                &session.factor_display_name,
                session.provider,
                session.local_id.as_deref(),
                session.email.as_deref(),
            )
        }
    }
}

/// Perform GitHub OAuth using Device Flow.
///
/// This flow is ideal for CLI tools as it doesn't require a callback server:
/// 1. Request a device code from GitHub
/// 2. User visits github.com/login/device and enters the code
/// 3. Poll GitHub until authorization completes
/// 4. Exchange the GitHub access token for Firebase tokens
fn authorize_github_device_flow(args: &LoginArgs) -> Result<OAuthTokens> {
    let client_id = get_embedded_default("github_client_id");

    println!("Using GitHub Device Flow authentication...");

    // Step 1: Request device and user codes
    let device_response = request_github_device_code(&client_id)?;

    // Step 2: Show user the code and URL
    println!();
    println!("  Visit: {}", device_response.verification_uri);
    println!("  Enter code: {}", device_response.user_code);
    println!();

    // Open browser if requested
    if !args.no_browser && open_browser(&device_response.verification_uri).is_err() {
        println!("Could not open browser automatically.");
    }

    println!(
        "Waiting for authorization (expires in {} seconds)...",
        device_response.expires_in
    );

    // Step 3: Poll for access token
    let access_token = poll_github_access_token(
        &client_id,
        &device_response.device_code,
        device_response.interval,
        device_response.expires_in,
    )?;

    println!("GitHub authorization successful, exchanging for Firebase token...");

    // Step 4: Exchange GitHub access token for Firebase tokens
    exchange_github_token_for_firebase(&access_token)
}

/// Request device and user codes from GitHub
fn request_github_device_code(client_id: &str) -> Result<GitHubDeviceCodeResponse> {
    let body = format!(
        "client_id={}&scope={}",
        urlencoding::encode(client_id),
        urlencoding::encode("user:email")
    );

    let response = minreq::post(GITHUB_DEVICE_CODE)
        .with_header("User-Agent", user_agent())
        .with_header("Accept", "application/json")
        .with_body(body)
        .with_header("Content-Type", "application/x-www-form-urlencoded")
        .with_timeout(10)
        .send()
        .context("Failed to request GitHub device code")?;

    if response.status_code != 200 {
        let body = response.as_str().unwrap_or("(non-utf8 response)");
        error!(
            "GitHub device code request failed: HTTP {} - {}",
            response.status_code, body
        );
        return Err(anyhow!(
            "Failed to request GitHub device code: HTTP {}",
            response.status_code
        ));
    }

    let data: GitHubDeviceCodeResponse = serde_json::from_str(
        response
            .as_str()
            .context("Invalid UTF-8 in GitHub response")?,
    )
    .context("Failed to parse GitHub device code response")?;

    info!("Got device code from GitHub, user code: {}", data.user_code);

    Ok(data)
}

/// Poll GitHub for access token until user authorizes or timeout
fn poll_github_access_token(
    client_id: &str,
    device_code: &str,
    initial_interval: u64,
    expires_in: u64,
) -> Result<String> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(expires_in);
    let mut interval = initial_interval.max(GITHUB_POLL_INTERVAL);

    loop {
        // Check if we've exceeded the timeout
        if start.elapsed() > timeout {
            return Err(anyhow!("GitHub authorization timed out. Please try again."));
        }

        // Wait before polling
        thread::sleep(Duration::from_secs(interval));

        let body = format!(
            "client_id={}&device_code={}&grant_type={}",
            urlencoding::encode(client_id),
            urlencoding::encode(device_code),
            urlencoding::encode("urn:ietf:params:oauth:grant-type:device_code")
        );

        let response = minreq::post(GITHUB_ACCESS_TOKEN)
            .with_header("User-Agent", user_agent())
            .with_header("Accept", "application/json")
            .with_body(body)
            .with_header("Content-Type", "application/x-www-form-urlencoded")
            .with_timeout(10)
            .send()
            .context("Failed to poll GitHub for access token")?;

        let body = response
            .as_str()
            .context("Invalid UTF-8 in GitHub response")?;
        let data: GitHubAccessTokenResponse =
            serde_json::from_str(body).context("Failed to parse GitHub access token response")?;

        // Check for access token (success)
        if let Some(token) = data.access_token {
            return Ok(token);
        }

        // Handle errors
        if let Some(error) = data.error {
            match error.as_str() {
                // User hasn't authorized yet, continue polling
                "authorization_pending" => {}
                "slow_down" => {
                    // Rate limited, increase interval by 5 seconds
                    interval = interval.saturating_add(5);
                    warn!("GitHub rate limit hit, increasing poll interval to {interval}s");
                }
                "expired_token" => {
                    return Err(anyhow!(
                        "Authorization expired. Please run the command again to get a new code."
                    ));
                }
                "access_denied" => {
                    return Err(anyhow!("Authorization was denied by the user."));
                }
                _ => {
                    let desc = data.error_description.unwrap_or_default();
                    return Err(anyhow!("GitHub authorization failed: {error} - {desc}"));
                }
            }
        }
    }
}

/// Exchange GitHub access token for Firebase tokens
fn exchange_github_token_for_firebase(github_access_token: &str) -> Result<OAuthTokens> {
    let api_key = get_embedded_default("firebase_api_key");
    let url = format!("{SIGN_IN_WITH_IDP}?key={api_key}");

    info!("Exchanging GitHub token with Firebase signInWithIdp");

    // For OAuth credential exchange, we use access_token in post_body
    let post_body = format!(
        "access_token={}&providerId=github.com",
        urlencoding::encode(github_access_token)
    );

    let payload = SignInWithCredentialRequest {
        post_body,
        request_uri: "http://localhost".to_string(), // Required but not used for credential exchange
        return_secure_token: true,
        return_idp_credential: true,
    };

    let response = minreq::post(&url)
        .with_header("User-Agent", user_agent())
        .with_json(&payload)?
        .with_timeout(10)
        .send()
        .context("Failed to exchange GitHub token with Firebase")?;

    let response_body = response.as_str().unwrap_or("(non-utf8 response)");

    if response.status_code != 200 {
        error!(
            "Firebase signInWithIdp failed: HTTP {} - {}",
            response.status_code, response_body
        );
        return Err(anyhow!(
            "Failed to exchange GitHub token with Firebase: HTTP {}",
            response.status_code
        ));
    }

    // Check if MFA is required
    if let Ok(mfa_response) = serde_json::from_str::<SignInWithIdpMfaResponse>(response_body) {
        info!("MFA required for GitHub auth");

        // Find TOTP factor
        let factor = mfa_response
            .mfa_info
            .iter()
            .find(|f| f.totp_info.is_some())
            .ok_or_else(|| {
                anyhow!(
                    "MFA is required but only TOTP (authenticator app) is supported.\n\
                     Please ensure you have an authenticator app configured."
                )
            })?;

        let factor_name = factor
            .display_name
            .clone()
            .unwrap_or_else(|| "your authenticator app".to_string());

        return handle_mfa_in_cli(
            &mfa_response.mfa_pending_credential,
            &factor.mfa_enrollment_id,
            &factor_name,
            OAuthProvider::GitHub,
            mfa_response.local_id.as_deref(),
            mfa_response.email.as_deref(),
        );
    }

    // Parse successful response
    let data: SignInWithIdpResponse = serde_json::from_str(response_body)
        .context("Failed to parse Firebase signInWithIdp response")?;

    // Calculate expiry timestamp
    let expires_in: i64 = data.expires_in.parse().unwrap_or(3600);
    #[allow(clippy::cast_possible_wrap)]
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let expires_at = now.saturating_add(expires_in);

    info!("Firebase token exchange successful");

    Ok(OAuthTokens {
        id_token: data.id_token,
        refresh_token: data.refresh_token,
        expires_at,
        provider: OAuthProvider::GitHub.as_firebase_id().to_string(),
        uid: data.local_id,
        email: data.email,
    })
}

/// Handle MFA verification in the CLI by prompting for TOTP code.
///
/// Allows up to 3 attempts for entering the correct code.
fn handle_mfa_in_cli(
    mfa_pending_credential: &str,
    mfa_enrollment_id: &str,
    factor_name: &str,
    provider: OAuthProvider,
    local_id: Option<&str>,
    email: Option<&str>,
) -> Result<OAuthTokens> {
    const MAX_ATTEMPTS: u8 = 3;

    println!("\nMulti-factor authentication required.");
    println!("Enter the 6-digit code from {factor_name}.");

    for attempt in 1..=MAX_ATTEMPTS {
        let prompt = if attempt == 1 {
            "MFA Code".to_string()
        } else {
            format!("MFA Code (attempt {attempt}/{MAX_ATTEMPTS})")
        };

        let validator = |input: &str| {
            if input.len() == 6 && input.chars().all(|c| c.is_ascii_digit()) {
                ValidationResult::Valid
            } else {
                ValidationResult::Invalid("Code must be exactly 6 digits".into())
            }
        };
        let code = text_prompt(
            &prompt,
            Some("Enter 6-digit code, Esc to cancel"),
            Some(&validator),
        )
        .map_err(|e| anyhow!("MFA input error: {e}"))?;

        let Some(code) = code else {
            return Err(anyhow!("MFA input cancelled"));
        };

        match finalize_mfa(
            mfa_pending_credential,
            mfa_enrollment_id,
            &code,
            provider,
            local_id,
            email,
        ) {
            Ok(tokens) => {
                println!("MFA verification successful.");
                return Ok(tokens);
            }
            Err(e) => {
                // Allow retry only for invalid code errors and if we have attempts left
                if is_retryable_mfa_error(&e) && attempt < MAX_ATTEMPTS {
                    println!("Invalid code. Please try again.");
                    continue;
                }
                return Err(e);
            }
        }
    }

    // This is technically unreachable since the loop always returns,
    // but serves as a safety net if the logic changes
    Err(anyhow!("Too many failed MFA attempts."))
}

/// Check if an MFA error is retryable (invalid/expired code)
fn is_retryable_mfa_error(e: &anyhow::Error) -> bool {
    let error_msg = e.to_string();
    error_msg.contains("Invalid or expired")
}

/// Find a free port from the preferred list
fn find_free_port() -> Result<u16> {
    for &port in PREFERRED_PORTS {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }

    Err(anyhow!(
        "All OAuth callback ports (8085-8089) are currently in use.\n\
         Please free up one of these ports or close applications using them."
    ))
}

/// Run the OAuth callback server
///
/// This server handles:
/// 1. The initial OAuth callback with authorization code
/// 2. Token exchange with Firebase
/// 3. MFA verification form (if MFA is required)
/// 4. MFA code submission and verification
#[allow(clippy::too_many_lines)]
fn run_callback_server(
    server: &Server,
    tx: &Sender<CallbackResult>,
    redirect_uri: &str,
    provider: OAuthProvider,
    session_id: &str,
) {
    let start_time = Instant::now();
    let timeout = Duration::from_secs(OAUTH_CALLBACK_TIMEOUT);

    // MFA session state - populated if MFA is required
    let mut mfa_session: Option<MfaSession> = None;

    for mut request in server.incoming_requests() {
        // Check timeout
        if start_time.elapsed() > timeout {
            let _ = tx.send(CallbackResult::Timeout);
            return;
        }

        let url = request.url().to_string();
        let method = request.method().to_string();

        // Handle favicon requests
        if url == "/favicon.ico" {
            let response = Response::empty(204);
            let _ = request.respond(response);
            continue;
        }

        // Handle MFA code submission (POST /mfa/verify)
        if method == "POST" && url == "/mfa/verify" {
            if let Some(ref session) = mfa_session {
                // Read the POST body
                let mut body = String::new();
                if let Err(e) = request.as_reader().read_to_string(&mut body) {
                    error!("Failed to read MFA POST body: {e}");
                    let _ = request.respond(text_response(400, "Failed to read request body"));
                    continue;
                }

                // Parse the code from form data (code=XXXXXX)
                let code = body
                    .split('&')
                    .find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        if parts.next() == Some("code") {
                            parts
                                .next()
                                .map(|v| urlencoding::decode(v).unwrap_or_default().into_owned())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();

                // Validate code format
                if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
                    let _ = request.respond(text_response(400, "Invalid code format"));
                    continue;
                }

                // Attempt MFA verification
                match finalize_mfa(
                    &session.mfa_pending_credential,
                    &session.mfa_enrollment_id,
                    &code,
                    session.provider,
                    session.local_id.as_deref(),
                    session.email.as_deref(),
                ) {
                    Ok(tokens) => {
                        // MFA successful - respond with success page HTML
                        let _ = request.respond(html_response(200, &success_html()));
                        let _ = tx.send(CallbackResult::Success(tokens));
                        return;
                    }
                    Err(e) => {
                        // MFA failed - return error message
                        let error_msg = e.to_string();
                        warn!("MFA verification failed: {error_msg}");
                        let _ = request.respond(text_response(401, &error_msg));
                        continue;
                    }
                }
            }
            let _ = request.respond(text_response(400, "No MFA session active"));
            continue;
        }

        // Handle MFA success page (GET /mfa/success)
        if url == "/mfa/success" {
            let _ = request.respond(html_response(200, &success_html()));
            continue;
        }

        // Parse the callback URL (GET /callback?...)
        if url.starts_with("/callback") {
            if let Ok(parsed) = Url::parse(&format!("http://localhost{url}")) {
                let params: HashMap<_, _> = parsed.query_pairs().collect();

                if let Some(error) = params.get("error") {
                    let error_desc = params
                        .get("error_description")
                        .map_or_else(|| error.to_string(), ToString::to_string);

                    let _ = request.respond(html_response(200, &error_html(&error_desc)));
                    let _ = tx.send(CallbackResult::Error(error_desc));
                    return;
                }

                if params.contains_key("code") {
                    // Extract query string for Firebase
                    let query_string = parsed.query().unwrap_or("").to_string();

                    // Exchange the authorization code for tokens
                    match exchange_code_for_tokens(
                        redirect_uri,
                        &query_string,
                        session_id,
                        provider,
                    ) {
                        Ok(ExchangeResult::Success(tokens)) => {
                            // No MFA required - show success and return tokens
                            let _ = request.respond(html_response(200, &success_html()));
                            let _ = tx.send(CallbackResult::Success(tokens));
                            return;
                        }
                        Ok(ExchangeResult::MfaRequired(session)) => {
                            // MFA required - show MFA form
                            info!("MFA required, showing browser form");
                            let _ = request.respond(html_response(
                                200,
                                &mfa_html(&session.factor_display_name),
                            ));
                            mfa_session = Some(session);
                            continue;
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            error!("Token exchange failed: {error_msg}");
                            let _ = request.respond(html_response(200, &error_html(&error_msg)));
                            let _ = tx.send(CallbackResult::Error(error_msg));
                            return;
                        }
                    }
                }
            }

            // Invalid callback - no code or error
            let _ = request.respond(text_response(400, "Invalid OAuth callback"));
            let _ = tx.send(CallbackResult::Error("Invalid OAuth callback".to_string()));
            return;
        }

        // Unknown request
        let _ = request.respond(text_response(404, "Not found"));
    }
}

/// Create a plain text HTTP response
#[allow(clippy::expect_used)]
fn text_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..])
                .expect("valid header"),
        )
}

/// Create an HTML HTTP response
#[allow(clippy::expect_used)]
fn html_response(status: u16, html: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(html)
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                .expect("valid header"),
        )
}

/// Result of exchanging the authorization code
enum ExchangeResult {
    /// Tokens received, no MFA required
    Success(OAuthTokens),
    /// MFA is required, session created
    MfaRequired(MfaSession),
}

/// Exchange authorization code for tokens, detecting if MFA is required
fn exchange_code_for_tokens(
    request_uri: &str,
    query_string: &str,
    session_id: &str,
    provider: OAuthProvider,
) -> Result<ExchangeResult> {
    let api_key = get_embedded_default("firebase_api_key");
    let url = format!("{SIGN_IN_WITH_IDP}?key={api_key}");

    info!("Exchanging OAuth callback with Firebase signInWithIdp");

    let payload = SignInWithIdpRequest {
        request_uri: request_uri.to_string(),
        post_body: query_string.to_string(),
        session_id: session_id.to_string(),
        return_secure_token: true,
        return_idp_credential: true,
    };

    let response = minreq::post(&url)
        .with_header("User-Agent", user_agent())
        .with_json(&payload)?
        .with_timeout(10)
        .send()
        .context("Failed to sign in with IdP")?;

    let response_body = response.as_str().unwrap_or("(non-utf8 response)");

    if response.status_code != 200 {
        error!("signInWithIdp failed: HTTP {}", response.status_code);
        return Err(anyhow!(
            "Failed to sign in with IdP: HTTP {}",
            response.status_code
        ));
    }

    // First, check if MFA is required by looking for mfaPendingCredential
    if let Ok(mfa_response) = serde_json::from_str::<SignInWithIdpMfaResponse>(response_body) {
        info!("MFA required, creating MFA session for browser verification");

        // Find the first TOTP factor
        let factor = mfa_response
            .mfa_info
            .iter()
            .find(|f| f.totp_info.is_some())
            .ok_or_else(|| {
                anyhow!("Only TOTP (authenticator app) MFA is supported. SMS MFA is not yet implemented.")
            })?;

        let session = MfaSession {
            mfa_pending_credential: mfa_response.mfa_pending_credential,
            mfa_enrollment_id: factor.mfa_enrollment_id.clone(),
            factor_display_name: factor
                .display_name
                .clone()
                .unwrap_or_else(|| "your authenticator app".to_string()),
            provider,
            local_id: mfa_response.local_id,
            email: mfa_response.email,
        };

        return Ok(ExchangeResult::MfaRequired(session));
    }

    // No MFA required, parse as regular response
    let data: SignInWithIdpResponse =
        serde_json::from_str(response_body).context("Failed to parse signInWithIdp response")?;

    // Calculate expiry timestamp
    let expires_in: i64 = data.expires_in.parse().unwrap_or(3600);
    #[allow(clippy::cast_possible_wrap)]
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let expires_at = now.saturating_add(expires_in);

    Ok(ExchangeResult::Success(OAuthTokens {
        id_token: data.id_token,
        refresh_token: data.refresh_token,
        expires_at,
        provider: provider.as_firebase_id().to_string(),
        uid: data.local_id,
        email: data.email,
    }))
}

/// Request auth URI from Firebase
fn create_auth_uri(provider: OAuthProvider, redirect_uri: &str) -> Result<(String, String)> {
    let api_key = get_embedded_default("firebase_api_key");
    let url = format!("{CREATE_AUTH_URI}?key={api_key}");

    info!(
        "Creating auth URI for provider: {}",
        provider.as_firebase_id()
    );

    let payload = CreateAuthUriRequest {
        provider_id: provider.as_firebase_id().to_string(),
        continue_uri: redirect_uri.to_string(),
        auth_flow_type: "CODE_FLOW".to_string(),
        oauth_scope: "openid email profile".to_string(),
    };

    let response = minreq::post(&url)
        .with_header("User-Agent", user_agent())
        .with_json(&payload)?
        .with_timeout(10)
        .send()
        .context("Failed to create auth URI")?;

    let response_body = response.as_str().unwrap_or("(non-utf8 response)");

    if response.status_code != 200 {
        error!("createAuthUri failed: HTTP {}", response.status_code);
        return Err(anyhow!(
            "Failed to create auth URI: HTTP {}",
            response.status_code
        ));
    }

    let data: CreateAuthUriResponse =
        serde_json::from_str(response_body).context("Failed to parse createAuthUri response")?;

    info!("Got auth URI from Firebase");

    Ok((data.session_id, data.auth_uri))
}

/// Finalize MFA verification and get tokens
fn finalize_mfa(
    mfa_pending_credential: &str,
    mfa_enrollment_id: &str,
    verification_code: &str,
    provider: OAuthProvider,
    local_id: Option<&str>,
    email: Option<&str>,
) -> Result<OAuthTokens> {
    let api_key = get_embedded_default("firebase_api_key");
    let url = format!("{MFA_FINALIZE}?key={api_key}");

    info!("Finalizing MFA verification");

    let payload = MfaFinalizeRequest {
        mfa_pending_credential: mfa_pending_credential.to_string(),
        mfa_enrollment_id: mfa_enrollment_id.to_string(),
        totp_verification_info: Some(TotpVerificationInfo {
            verification_code: verification_code.to_string(),
        }),
    };

    let response = minreq::post(&url)
        .with_header("User-Agent", user_agent())
        .with_json(&payload)?
        .with_timeout(15)
        .send()
        .context("Failed to finalize MFA")?;

    let response_body = response.as_str().unwrap_or("(non-utf8 response)");

    if response.status_code != 200 {
        // Parse error message
        let error_msg =
            if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(response_body) {
                error_json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error")
                    .to_string()
            } else {
                response_body.to_string()
            };

        if error_msg.contains("INVALID_CODE") || error_msg.contains("CODE_EXPIRED") {
            return Err(anyhow!("Invalid or expired verification code."));
        } else if error_msg.contains("INVALID_MFA_PENDING_CREDENTIAL") {
            return Err(anyhow!("MFA session expired. Please try logging in again."));
        } else if error_msg.contains("TOO_MANY_ATTEMPTS") {
            return Err(anyhow!(
                "Too many failed attempts. Please try logging in again."
            ));
        }

        return Err(anyhow!("MFA verification failed: {error_msg}"));
    }

    let data: MfaFinalizeResponse =
        serde_json::from_str(response_body).context("Failed to parse MFA finalize response")?;

    info!("MFA verification successful");

    // Calculate expiry timestamp (default to 1 hour if not provided)
    let expires_in: i64 = data
        .expires_in
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    #[allow(clippy::cast_possible_wrap)]
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let expires_at = now.saturating_add(expires_in);

    Ok(OAuthTokens {
        id_token: data.id_token,
        refresh_token: data.refresh_token,
        expires_at,
        provider: provider.as_firebase_id().to_string(),
        uid: local_id.map(String::from),
        email: email.map(String::from),
    })
}

/// Open URL in the default browser
pub fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("Failed to open browser")?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("Failed to open browser")?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .context("Failed to open browser")?;
    }

    Ok(())
}

/// Generate success HTML page
fn success_html() -> String {
    OAuthAssets::get("success.html").map_or_else(
        || "Authentication successful! You can close this window.".to_string(),
        |file| String::from_utf8_lossy(&file.data).into_owned(),
    )
}

/// Generate MFA HTML page with TOTP input form
fn mfa_html(factor_name: &str) -> String {
    // HTML-escape the factor name to prevent XSS
    let escaped_name = html_escape(factor_name);
    OAuthAssets::get("mfa.html").map_or_else(
        || format!("MFA required. Enter the 6-digit code from {escaped_name}."),
        |file| String::from_utf8_lossy(&file.data).replace("{{FACTOR_NAME}}", &escaped_name),
    )
}

/// Generate error HTML page
fn error_html(error_msg: &str) -> String {
    // HTML-escape the error message to prevent XSS
    let escaped_msg = html_escape(error_msg);
    OAuthAssets::get("error.html").map_or_else(
        || format!("Authentication failed: {escaped_msg}"),
        |file| String::from_utf8_lossy(&file.data).replace("{{ERROR_MESSAGE}}", &escaped_msg),
    )
}

/// Escape HTML special characters to prevent XSS
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_provider_firebase_ids() {
        assert_eq!(OAuthProvider::Google.as_firebase_id(), "google.com");
        assert_eq!(OAuthProvider::Microsoft.as_firebase_id(), "microsoft.com");
        assert_eq!(OAuthProvider::GitHub.as_firebase_id(), "github.com");
    }

    #[test]
    fn test_oauth_provider_equality() {
        assert_eq!(OAuthProvider::GitHub, OAuthProvider::GitHub);
        assert_ne!(OAuthProvider::GitHub, OAuthProvider::Google);
        assert_ne!(OAuthProvider::GitHub, OAuthProvider::Microsoft);
    }

    #[test]
    fn test_github_device_code_response_deserialization() {
        let json = r#"{
            "device_code": "abc123",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }"#;

        let response: GitHubDeviceCodeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.device_code, "abc123");
        assert_eq!(response.user_code, "ABCD-1234");
        assert_eq!(response.verification_uri, "https://github.com/login/device");
        assert_eq!(response.expires_in, 900);
        assert_eq!(response.interval, 5);
    }

    #[test]
    fn test_github_access_token_response_success() {
        let json = r#"{
            "access_token": "gho_xxxxxxxxxxxx",
            "token_type": "bearer",
            "scope": "user:email"
        }"#;

        let response: GitHubAccessTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.access_token, Some("gho_xxxxxxxxxxxx".to_string()));
        assert!(response.error.is_none());
    }

    #[test]
    fn test_github_access_token_response_pending() {
        let json = r#"{
            "error": "authorization_pending",
            "error_description": "The authorization request is still pending."
        }"#;

        let response: GitHubAccessTokenResponse = serde_json::from_str(json).unwrap();
        assert!(response.access_token.is_none());
        assert_eq!(response.error, Some("authorization_pending".to_string()));
    }

    #[test]
    fn test_github_access_token_response_slow_down() {
        let json = r#"{
            "error": "slow_down",
            "error_description": "Too many requests.",
            "interval": 10
        }"#;

        let response: GitHubAccessTokenResponse = serde_json::from_str(json).unwrap();
        assert!(response.access_token.is_none());
        assert_eq!(response.error, Some("slow_down".to_string()));
    }

    #[test]
    fn test_github_access_token_response_expired() {
        let json = r#"{
            "error": "expired_token",
            "error_description": "The device_code has expired."
        }"#;

        let response: GitHubAccessTokenResponse = serde_json::from_str(json).unwrap();
        assert!(response.access_token.is_none());
        assert_eq!(response.error, Some("expired_token".to_string()));
    }

    #[test]
    fn test_github_access_token_response_access_denied() {
        let json = r#"{
            "error": "access_denied",
            "error_description": "The user has denied your application access."
        }"#;

        let response: GitHubAccessTokenResponse = serde_json::from_str(json).unwrap();
        assert!(response.access_token.is_none());
        assert_eq!(response.error, Some("access_denied".to_string()));
    }

    #[test]
    fn test_sign_in_with_credential_request_serialization() {
        let request = SignInWithCredentialRequest {
            post_body: "access_token=abc123&providerId=github.com".to_string(),
            request_uri: "http://localhost".to_string(),
            return_secure_token: true,
            return_idp_credential: true,
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(
            json["postBody"],
            "access_token=abc123&providerId=github.com"
        );
        assert_eq!(json["requestUri"], "http://localhost");
        assert_eq!(json["returnSecureToken"], true);
        assert_eq!(json["returnIdpCredential"], true);
    }

    #[test]
    fn test_mfa_response_deserialization() {
        // This tests that we can correctly parse an MFA-required response
        let json = r#"{
            "mfaPendingCredential": "pending-cred-123",
            "mfaInfo": [
                {
                    "mfaEnrollmentId": "enrollment-456",
                    "displayName": "My Authenticator",
                    "totpInfo": {}
                }
            ],
            "localId": "user-789",
            "email": "user@example.com"
        }"#;

        let response: SignInWithIdpMfaResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.mfa_pending_credential, "pending-cred-123");
        assert_eq!(response.mfa_info.len(), 1);
        assert_eq!(response.mfa_info[0].mfa_enrollment_id, "enrollment-456");
        assert_eq!(
            response.mfa_info[0].display_name,
            Some("My Authenticator".to_string())
        );
        assert!(response.mfa_info[0].totp_info.is_some());
        assert_eq!(response.local_id, Some("user-789".to_string()));
        assert_eq!(response.email, Some("user@example.com".to_string()));
    }

    #[test]
    fn test_mfa_response_not_parsed_from_success_response() {
        // A successful response should NOT parse as MFA response
        let json = r#"{
            "idToken": "id-token-123",
            "refreshToken": "refresh-token-456",
            "expiresIn": "3600",
            "localId": "user-789",
            "email": "user@example.com"
        }"#;

        // This should fail to parse as MFA response (missing mfaPendingCredential)
        let result = serde_json::from_str::<SignInWithIdpMfaResponse>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_retryable_mfa_error_invalid_code() {
        let error = anyhow!("Invalid or expired verification code.");
        assert!(is_retryable_mfa_error(&error));
    }

    #[test]
    fn test_is_retryable_mfa_error_session_expired() {
        let error = anyhow!("MFA session expired. Please try logging in again.");
        assert!(!is_retryable_mfa_error(&error));
    }

    #[test]
    fn test_is_retryable_mfa_error_too_many_attempts() {
        let error = anyhow!("Too many failed attempts. Please try logging in again.");
        assert!(!is_retryable_mfa_error(&error));
    }

    #[test]
    fn test_callback_url_parsing_with_code() {
        let url = "http://localhost:8085/callback?code=abc123&state=xyz";
        let parsed = Url::parse(url).unwrap();
        let params: HashMap<_, _> = parsed.query_pairs().collect();

        assert!(params.contains_key("code"));
        assert_eq!(params.get("code").map(|s| s.as_ref()), Some("abc123"));
    }

    #[test]
    fn test_callback_url_parsing_with_error() {
        let url =
            "http://localhost:8085/callback?error=access_denied&error_description=User%20denied";
        let parsed = Url::parse(url).unwrap();
        let params: HashMap<_, _> = parsed.query_pairs().collect();

        assert!(params.contains_key("error"));
        assert_eq!(
            params.get("error").map(|s| s.as_ref()),
            Some("access_denied")
        );
        assert_eq!(
            params.get("error_description").map(|s| s.as_ref()),
            Some("User denied")
        );
    }

    #[test]
    fn test_callback_url_parsing_extracts_query_string() {
        let url = "http://localhost:8085/callback?code=abc123&state=xyz&scope=email";
        let parsed = Url::parse(url).unwrap();
        let query = parsed.query().unwrap();

        assert!(query.contains("code=abc123"));
        assert!(query.contains("state=xyz"));
    }

    #[test]
    fn test_callback_url_parsing_path_only() {
        // Users might paste just the path portion
        let path = "/callback?code=abc123&state=xyz";
        let url = format!("http://localhost{path}");
        let parsed = Url::parse(&url).unwrap();
        let params: HashMap<_, _> = parsed.query_pairs().collect();

        assert!(params.contains_key("code"));
        assert_eq!(params.get("code").map(|s| s.as_ref()), Some("abc123"));
    }

    #[test]
    fn test_is_browser_available_returns_bool() {
        // This test just verifies the function runs without panicking
        // The actual result depends on the environment
        let result = is_browser_available();
        // Result is a boolean
        assert!(result || !result);
    }
}
