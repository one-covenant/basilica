//! OAuth callback server for CLI authentication
//!
//! This module provides a local HTTP server to receive OAuth callbacks
//! when using browser-based authentication flow in CLI applications.

use crate::models::auth::AuthError;
use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use html_escape;
use serde::Deserialize;
use std::net::{SocketAddr, TcpListener};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener as TokioTcpListener;

/// Authorization callback data received from OAuth provider
#[derive(Debug, Clone)]
pub struct CallbackData {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Query parameters from OAuth callback
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Shared state for callback handling
#[derive(Debug)]
struct CallbackState {
    sender: mpsc::Sender<CallbackData>,
    expected_state: String,
}

/// Local HTTP server for OAuth callbacks
pub struct CallbackServer {
    port: u16,
    timeout: Duration,
}

impl CallbackServer {
    /// Create a new callback server
    pub fn new(port: u16, timeout: Duration) -> Self {
        Self { port, timeout }
    }

    /// Create with default settings (port 8080, 5 minute timeout)
    pub fn default() -> Self {
        Self::new(8080, Duration::from_secs(300))
    }

    /// Find an available port for the callback server
    pub fn find_available_port() -> Result<u16, AuthError> {
        // Try port 8080 first, then find any available port
        let preferred_port = 8080;

        match TcpListener::bind(("127.0.0.1", preferred_port)) {
            Ok(listener) => {
                let port = listener
                    .local_addr()
                    .map_err(|e| {
                        AuthError::NetworkError(format!("Failed to get local address: {}", e))
                    })?
                    .port();
                drop(listener);
                Ok(port)
            }
            Err(_) => {
                // Port 8080 is occupied, find any available port
                let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| {
                    AuthError::NetworkError(format!("Failed to bind to any port: {}", e))
                })?;
                let port = listener
                    .local_addr()
                    .map_err(|e| {
                        AuthError::NetworkError(format!("Failed to get local address: {}", e))
                    })?
                    .port();
                drop(listener);
                Ok(port)
            }
        }
    }

    /// Start the callback server and wait for OAuth response
    pub async fn start_and_wait(&self, expected_state: &str) -> Result<CallbackData, AuthError> {
        let (tx, rx) = mpsc::channel();

        // Create shared state
        let callback_state = Arc::new(Mutex::new(CallbackState {
            sender: tx,
            expected_state: expected_state.to_string(),
        }));

        // Create the router
        let app = Router::new()
            .route("/callback", get(handle_callback))
            .route("/auth/callback", get(handle_callback))
            .route("/oauth/callback", get(handle_callback))
            .with_state(callback_state.clone());

        // Create the server address
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));

        // Start the server
        let listener = TokioTcpListener::bind(&addr)
            .await
            .map_err(|e| AuthError::NetworkError(format!("Failed to bind to {}: {}", addr, e)))?;

        tracing::info!("OAuth callback server listening on http://{}", addr);

        // Start the server in a background task
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .map_err(|e| AuthError::NetworkError(format!("Server error: {}", e)))
        });

        // Wait for callback with timeout
        let result = tokio::select! {
            callback_result = tokio::task::spawn_blocking(move || rx.recv()) => {
                match callback_result {
                    Ok(Ok(data)) => Ok(data),
                    Ok(Err(_)) => Err(AuthError::NetworkError("Channel closed unexpectedly".to_string())),
                    Err(e) => Err(AuthError::NetworkError(format!("Task join error: {}", e))),
                }
            },
            _ = tokio::time::sleep(self.timeout) => {
                Err(AuthError::OAuthError("Authentication timeout".to_string()))
            }
        };

        // Abort the server
        server_handle.abort();

        result
    }

    /// Generate success HTML page to display to user
    pub fn generate_success_page() -> String {
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Authorization Successful - Basilica</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            margin: 0;
            padding: 0;
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
        }
        .container {
            background: #ffffff;
            padding: 48px;
            border-radius: 16px;
            box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1), 0 10px 10px -5px rgba(0, 0, 0, 0.04);
            max-width: 400px;
            width: 100%;
            text-align: center;
        }
        .success-icon {
            width: 72px;
            height: 72px;
            margin: 0 auto 24px;
            background: #10B981;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            color: white;
            font-size: 36px;
        }
        h1 {
            margin: 0 0 16px 0;
            font-size: 28px;
            font-weight: 700;
            color: #111827;
        }
        p {
            margin: 0 0 8px 0;
            font-size: 16px;
            color: #6B7280;
            line-height: 1.5;
        }
        .close-instruction {
            margin-top: 24px;
            padding-top: 24px;
            border-top: 1px solid #E5E7EB;
            font-size: 14px;
            color: #9CA3AF;
        }
        .code-block {
            background: #F3F4F6;
            border: 1px solid #E5E7EB;
            border-radius: 8px;
            padding: 12px;
            margin: 16px 0;
            font-family: 'SF Mono', Monaco, 'Courier New', monospace;
            font-size: 14px;
            color: #374151;
        }
    </style>
    <script>
        // Auto-close after 5 seconds
        setTimeout(() => {
            window.close();
        }, 5000);
    </script>
</head>
<body>
    <div class="container">
        <div class="success-icon">✓</div>
        <h1>Authentication Successful!</h1>
        <p>You have been successfully authenticated with Basilica.</p>
        <div class="code-block">You can now return to your terminal</div>
        <p class="close-instruction">This window will close automatically in 5 seconds...</p>
    </div>
</body>
</html>
        "#
        .to_string()
    }

    /// Generate error HTML page to display to user
    pub fn generate_error_page(error: &str) -> String {
        format!(
            r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Authorization Failed - Basilica</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            margin: 0;
            padding: 0;
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
        }}
        .container {{
            background: #ffffff;
            padding: 48px;
            border-radius: 16px;
            box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1), 0 10px 10px -5px rgba(0, 0, 0, 0.04);
            max-width: 400px;
            width: 100%;
            text-align: center;
        }}
        .error-icon {{
            width: 72px;
            height: 72px;
            margin: 0 auto 24px;
            background: #EF4444;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            color: white;
            font-size: 36px;
        }}
        h1 {{
            margin: 0 0 16px 0;
            font-size: 28px;
            font-weight: 700;
            color: #111827;
        }}
        p {{
            margin: 0 0 8px 0;
            font-size: 16px;
            color: #6B7280;
            line-height: 1.5;
        }}
        .error-details {{
            background: #FEF2F2;
            border: 1px solid #FEE2E2;
            padding: 12px;
            border-radius: 8px;
            margin: 16px 0;
            font-family: 'SF Mono', Monaco, 'Courier New', monospace;
            font-size: 14px;
            color: #991B1B;
            word-break: break-word;
            text-align: left;
        }}
        .close-instruction {{
            margin-top: 24px;
            padding-top: 24px;
            border-top: 1px solid #E5E7EB;
            font-size: 14px;
            color: #9CA3AF;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="error-icon">✗</div>
        <h1>Authentication Failed</h1>
        <p>We couldn't complete the authentication process.</p>
        <div class="error-details">{}</div>
        <p class="close-instruction">Please close this window and try again in your terminal.</p>
    </div>
</body>
</html>
        "#,
            html_escape::encode_text(error)
        )
    }
}

/// Axum handler for OAuth callback
async fn handle_callback(
    Query(params): Query<CallbackQuery>,
    axum::extract::State(state): axum::extract::State<Arc<Mutex<CallbackState>>>,
) -> impl IntoResponse {
    let _callback_data = CallbackData {
        code: params.code.clone(),
        state: params.state.clone(),
        error: params.error.clone(),
        error_description: params.error_description.clone(),
    };

    // Determine what to send and what HTML to show
    let (response_html, data_to_send) = if let Some(error) = &params.error {
        // OAuth error
        let error_msg = params.error_description.as_deref().unwrap_or(error);
        (
            CallbackServer::generate_error_page(error_msg),
            CallbackData {
                code: None,
                state: params.state.clone(),
                error: Some(error.to_string()),
                error_description: Some(error_msg.to_string()),
            },
        )
    } else if let Some(code) = params.code {
        // Validate state parameter
        let expected_state = state
            .lock()
            .map(|g| g.expected_state.clone())
            .unwrap_or_default();

        if params.state.as_deref() == Some(&expected_state) {
            // Success!
            (
                CallbackServer::generate_success_page(),
                CallbackData {
                    code: Some(code),
                    state: params.state.clone(),
                    error: None,
                    error_description: None,
                },
            )
        } else {
            // State mismatch
            let error_msg = format!(
                "Security validation failed. Expected state: {}, received: {:?}",
                expected_state, params.state
            );
            (
                CallbackServer::generate_error_page(&error_msg),
                CallbackData {
                    code: None,
                    state: params.state.clone(),
                    error: Some("state_mismatch".to_string()),
                    error_description: Some(error_msg),
                },
            )
        }
    } else {
        // No code received
        (
            CallbackServer::generate_error_page("No authorization code received"),
            CallbackData {
                code: None,
                state: params.state.clone(),
                error: Some("missing_code".to_string()),
                error_description: Some("No authorization code received".to_string()),
            },
        )
    };

    // Send the callback data through the channel
    if let Ok(state_guard) = state.lock() {
        let _ = state_guard.sender.send(data_to_send);
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(response_html),
    )
}

/// Helper function to perform browser-based OAuth flow
pub async fn browser_auth_flow(
    auth_service: &impl crate::services::auth::AuthService,
    state: &str,
) -> Result<crate::models::auth::TokenSet, AuthError> {
    // Get auth URL with PKCE
    let (auth_url, pkce) = auth_service.get_auth_url(state).await?;

    // Find available port for callback server
    let port = CallbackServer::find_available_port()?;

    // Update redirect URI if needed (this would need to be coordinated with OAuth provider)
    let callback_server = CallbackServer::new(port, Duration::from_secs(300));

    // Open browser
    println!("Opening browser for authentication...");
    println!("If the browser doesn't open, please visit:");
    println!("{}", auth_url);

    // Try to open browser (optional - can fail in headless environments)
    let _ = webbrowser::open(&auth_url);

    // Start callback server and wait for response
    let callback_data = callback_server.start_and_wait(state).await?;

    // Check for errors
    if let Some(error) = callback_data.error {
        return Err(AuthError::OAuthError(format!(
            "Authentication failed: {} - {}",
            error,
            callback_data.error_description.unwrap_or_default()
        )));
    }

    // Exchange code for tokens
    let code = callback_data
        .code
        .ok_or_else(|| AuthError::OAuthError("No authorization code received".to_string()))?;

    auth_service.exchange_code(&code, &pkce.verifier).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_callback_data_creation() {
        let data = CallbackData {
            code: Some("test_code".to_string()),
            state: Some("test_state".to_string()),
            error: None,
            error_description: None,
        };

        assert_eq!(data.code, Some("test_code".to_string()));
        assert_eq!(data.state, Some("test_state".to_string()));
        assert!(data.error.is_none());
    }

    #[test]
    fn test_html_generation() {
        let success_html = CallbackServer::generate_success_page();
        assert!(success_html.contains("Authentication Successful"));

        let error_html = CallbackServer::generate_error_page("Test error");
        assert!(error_html.contains("Authentication Failed"));
        assert!(error_html.contains("Test error"));
    }

    #[tokio::test]
    async fn test_find_available_port() {
        let port = CallbackServer::find_available_port();
        assert!(port.is_ok());
        let port_num = port.unwrap();
        assert!(port_num > 0);
    }
}
