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

use crate::default::get_embedded_default;

#[derive(Embed)]
#[folder = "resources/oauth/"]
struct OAuthAssets;

/// Firebase Auth API endpoints
const CREATE_AUTH_URI: &str = "https://identitytoolkit.googleapis.com/v1/accounts:createAuthUri";
const SIGN_IN_WITH_IDP: &str = "https://identitytoolkit.googleapis.com/v1/accounts:signInWithIdp";
const MFA_FINALIZE: &str = "https://identitytoolkit.googleapis.com/v2/accounts/mfaSignIn:finalize";

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
}

impl OAuthProvider {
    fn as_firebase_id(self) -> &'static str {
        match self {
            Self::Google => "google.com",
            Self::Microsoft => "microsoft.com",
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
    /// OAuth provider to use
    #[arg(long, short, default_value = "google")]
    pub provider: OAuthProvider,

    /// Print the URL instead of opening a browser
    #[arg(long)]
    pub no_browser: bool,
}

/// Perform OAuth authorization flow.
///
/// This function:
/// 1. Starts a local HTTP server to receive the OAuth callback
/// 2. Requests an auth URI from Firebase
/// 3. Opens the browser (or prints the URL) for user authentication
/// 4. Waits for the callback with the authorization code
/// 5. Exchanges the code for Firebase tokens
///
/// # Arguments
/// * `config` - Configuration for the authorization flow
///
/// # Returns
/// * `Ok(OAuthTokens)` - The OAuth tokens on success
/// * `Err` - An error if authorization fails
pub fn authorize(config: &LoginArgs) -> Result<OAuthTokens> {
    // Find a free port and start the callback server
    let port = find_free_port()?;
    let redirect_uri = format!("http://localhost:{port}/callback");

    println!("OAuth callback server started on port {port}");
    println!("Using OAuth provider: {}", config.provider.as_firebase_id());

    // Get auth URI from Firebase first (we need session_id for the callback server)
    let (session_id, auth_uri) = create_auth_uri(config.provider, &redirect_uri)?;

    // Start the callback server in a separate thread
    let (tx, rx): (Sender<CallbackResult>, Receiver<CallbackResult>) = mpsc::channel();
    let server = Server::http(format!("127.0.0.1:{port}"))
        .map_err(|e| anyhow!("Failed to start OAuth callback server: {e}"))?;

    let redirect_uri_clone = redirect_uri.clone();
    let provider = config.provider;
    let server_handle = thread::spawn(move || {
        run_callback_server(&server, &tx, &redirect_uri_clone, provider, &session_id);
    });

    // Open browser or print URL
    if config.no_browser {
        println!("\nPlease visit this URL to authenticate:\n{auth_uri}\n");
    } else {
        println!("Opening browser for authentication...");
        if open_browser(&auth_uri).is_err() {
            println!("\nCould not open browser. Please visit this URL:\n{auth_uri}\n");
        }
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
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
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
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
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
fn open_browser(url: &str) -> Result<()> {
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
    OAuthAssets::get("mfa.html").map_or_else(
        || format!("MFA required. Enter the 6-digit code from {factor_name}."),
        |file| String::from_utf8_lossy(&file.data).replace("{{FACTOR_NAME}}", factor_name),
    )
}

/// Generate error HTML page
fn error_html(error_msg: &str) -> String {
    OAuthAssets::get("error.html").map_or_else(
        || format!("Authentication failed: {error_msg}"),
        |file| String::from_utf8_lossy(&file.data).replace("{{ERROR_MESSAGE}}", error_msg),
    )
}
