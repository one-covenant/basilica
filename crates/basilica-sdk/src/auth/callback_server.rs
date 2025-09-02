//! Local HTTP callback server for OAuth authorization code flow
//!
//! This module implements a temporary local HTTP server to receive
//! the authorization callback from the OAuth provider.

use super::types::{AuthError, AuthResult};
use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
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

/// Callback state shared between server and main flow
struct CallbackState {
    sender: mpsc::Sender<CallbackData>,
    expected_state: String,
}

/// OAuth callback query parameters
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Local HTTP server for handling OAuth callbacks
pub struct CallbackServer {
    port: u16,
    timeout: Duration,
}

impl CallbackServer {
    /// Create a new callback server on the specified port
    pub fn new(port: u16, timeout: Duration) -> Self {
        Self { port, timeout }
    }

    /// Find an available port in the typical OAuth callback range
    pub fn find_available_port() -> AuthResult<u16> {
        // Try common OAuth callback ports
        for port in [8080, 8081, 8082, 3000, 3001, 9000].iter() {
            if Self::is_port_available(*port) {
                return Ok(*port);
            }
        }

        // Try any available port
        for port in 8083..9000 {
            if Self::is_port_available(port) {
                return Ok(port);
            }
        }

        Err(AuthError::CallbackServerError(
            "No available ports for callback server".to_string(),
        ))
    }

    /// Check if a port is available
    fn is_port_available(port: u16) -> bool {
        TcpListener::bind(("127.0.0.1", port)).is_ok()
    }

    /// Start the server and wait for callback
    pub async fn start_and_wait(&self, expected_state: String) -> AuthResult<CallbackData> {
        let (tx, rx) = mpsc::channel();

        let state = Arc::new(Mutex::new(CallbackState {
            sender: tx,
            expected_state,
        }));

        // Create router
        let app = Router::new()
            .route("/callback", get(handle_callback))
            .with_state(state);

        // Bind to address
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let listener = TokioTcpListener::bind(addr).await.map_err(|e| {
            AuthError::CallbackServerError(format!("Failed to bind to {}: {}", addr, e))
        })?;

        // Run server in background
        let server_handle = tokio::spawn(async move { axum::serve(listener, app).await });

        // Wait for callback with timeout
        let timeout = self.timeout;
        let result = tokio::task::spawn_blocking(move || {
            rx.recv_timeout(timeout)
                .map_err(|_| AuthError::CallbackServerError("Callback timeout".to_string()))
        })
        .await
        .map_err(|e| {
            AuthError::CallbackServerError(format!("Failed to wait for callback: {}", e))
        })?;

        // Shutdown server
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
            background: #000000;
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
            border-radius: 8px;
            box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
            max-width: 500px;
            width: 100%;
            text-align: center;
        }
        .logo {
            width: 64px;
            height: 64px;
            margin: 0 auto 24px;
            display: block;
        }
        .success-icon {
            width: 64px;
            height: 64px;
            margin: 0 auto 24px;
            background: #10B981;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            color: white;
            font-size: 32px;
        }
        h1 {
            margin: 0 0 16px 0;
            font-size: 24px;
            font-weight: 600;
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
            font-size: 14px;
            color: #9CA3AF;
        }
    </style>
</head>
<body>
    <div class="container">
        <img src="https://www.synapz.org/assets/basilica/basilica_logo200x200.png" alt="Basilica" class="logo">
        <div class="success-icon">✓</div>
        <h1>Welcome to Basilica</h1>
        <p>Authentication successful!</p>
        <p class="close-instruction">You can now close this window and return to the CLI.</p>
    </div>
</body>
</html>
        "#.to_string()
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
    <title>Authorization Failed - Basilica CLI</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
            background: #000000;
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
            border-radius: 8px;
            box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
            max-width: 400px;
            width: 100%;
            text-align: center;
        }}
        .logo {{
            width: 64px;
            height: 64px;
            margin: 0 auto 24px;
            display: block;
        }}
        .error-icon {{
            width: 64px;
            height: 64px;
            margin: 0 auto 24px;
            background: #EF4444;
            border-radius: 50%;
            display: flex;
            align-items: center;
            justify-content: center;
            color: white;
            font-size: 32px;
        }}
        h1 {{
            margin: 0 0 16px 0;
            font-size: 24px;
            font-weight: 600;
            color: #111827;
        }}
        p {{
            margin: 0 0 8px 0;
            font-size: 16px;
            color: #6B7280;
            line-height: 1.5;
        }}
        .error-details {{
            background: #F9FAFB;
            border: 1px solid #E5E7EB;
            padding: 12px;
            border-radius: 6px;
            margin: 16px 0;
            font-family: 'SF Mono', Monaco, 'Courier New', monospace;
            font-size: 14px;
            color: #EF4444;
            word-break: break-word;
            text-align: left;
        }}
        .close-instruction {{
            margin-top: 24px;
            font-size: 14px;
            color: #9CA3AF;
        }}
    </style>
</head>
<body>
    <div class="container">
        <img src="https://www.synapz.org/assets/basilica/basilica_logo200x200.png" alt="Basilica" class="logo">
        <div class="error-icon">✗</div>
        <h1>Authorization Failed</h1>
        <div class="error-details">{}</div>
        <p class="close-instruction">Please close this window and try again in the CLI.</p>
    </div>
</body>
</html>
        "#,
            error
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\"', "&quot;")
        )
    }
}

/// Axum handler for OAuth callback
async fn handle_callback(
    Query(params): Query<CallbackQuery>,
    axum::extract::State(state): axum::extract::State<Arc<Mutex<CallbackState>>>,
) -> impl IntoResponse {
    let callback_data = CallbackData {
        code: params.code.clone(),
        state: params.state.clone(),
        error: params.error.clone(),
        error_description: params.error_description.clone(),
    };

    // If we need to notify the waiting task (errors), fill this and send after building HTML.
    let mut data_to_send: Option<CallbackData> = None;

    let response_html = if let Some(error) = &params.error {
        // Generate error page
        let error_msg = params.error_description.as_deref().unwrap_or(error);
        // Notify waiter with an error (no code)
        data_to_send = Some(CallbackData {
            code: None,
            state: params.state.clone(),
            error: Some(error.to_string()),
            error_description: Some(error_msg.to_string()),
        });
        CallbackServer::generate_error_page(error_msg)
    } else if params.code.is_some() {
        // Validate state parameter
        let expected_state = match state.lock() {
            Ok(g) => g.expected_state.clone(),
            Err(_) => {
                let error_data = CallbackData {
                    code: None,
                    state: params.state.clone(),
                    error: Some("internal_error".to_string()),
                    error_description: Some("Internal state unavailable".to_string()),
                };
                // Try to send error even if mutex is poisoned - there might be other senders
                if let Ok(state_guard) = state.lock() {
                    let _ = state_guard.sender.send(error_data);
                }
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    Html(CallbackServer::generate_error_page("Internal error")),
                );
            }
        };

        if let Some(received_state) = &params.state {
            if received_state != &expected_state {
                let error_msg = format!(
                    "State mismatch: expected {}, got {}",
                    expected_state, received_state
                );
                data_to_send = Some(CallbackData {
                    code: None,
                    state: params.state.clone(),
                    error: Some("state_mismatch".to_string()),
                    error_description: Some(error_msg.clone()),
                });
                CallbackServer::generate_error_page(&error_msg)
            } else {
                // Send the callback data through the channel
                if let Ok(state_guard) = state.lock() {
                    let _ = state_guard.sender.send(callback_data);
                }
                CallbackServer::generate_success_page()
            }
        } else {
            data_to_send = Some(CallbackData {
                code: None,
                state: None,
                error: Some("missing_state".to_string()),
                error_description: Some("Missing state parameter".to_string()),
            });
            CallbackServer::generate_error_page("Missing state parameter")
        }
    } else {
        data_to_send = Some(CallbackData {
            code: None,
            state: params.state.clone(),
            error: Some("missing_code".to_string()),
            error_description: Some("Missing authorization code".to_string()),
        });
        CallbackServer::generate_error_page("Missing authorization code")
    };

    if let Some(to_send) = data_to_send {
        if let Ok(state_guard) = state.lock() {
            let _ = state_guard.sender.send(to_send);
        }
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(response_html),
    )
}
