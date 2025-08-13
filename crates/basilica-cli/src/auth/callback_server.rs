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
// CORS not needed for localhost callback server

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

    /// Find an available port for the callback server
    pub fn find_available_port() -> AuthResult<u16> {
        // Try port 8080 first, then find any available port
        let preferred_port = 8080;

        match TcpListener::bind(("127.0.0.1", preferred_port)) {
            Ok(listener) => {
                let port = listener.local_addr()?.port();
                drop(listener);
                Ok(port)
            }
            Err(_) => {
                // Port 8080 is occupied, find any available port
                let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| {
                    AuthError::CallbackServerError(format!("Failed to bind to any port: {}", e))
                })?;
                let port = listener.local_addr()?.port();
                drop(listener);
                Ok(port)
            }
        }
    }

    /// Start the callback server and wait for OAuth response
    pub async fn start_and_wait(&self, expected_state: &str) -> AuthResult<CallbackData> {
        let (tx, rx) = mpsc::channel();

        // Create shared state
        let callback_state = Arc::new(Mutex::new(CallbackState {
            sender: tx,
            expected_state: expected_state.to_string(),
        }));

        // Create the router - CORS not needed for localhost
        let app = Router::new()
            .route("/callback", get(handle_callback))
            .route("/auth/callback", get(handle_callback))
            .with_state(callback_state.clone());

        // Create the server address
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));

        // Start the server
        let listener = TokioTcpListener::bind(&addr).await.map_err(|e| {
            AuthError::CallbackServerError(format!("Failed to bind to {}: {}", addr, e))
        })?;

        tracing::info!("OAuth callback server listening on http://{}", addr);

        // Start the server in a background task
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .map_err(|e| AuthError::CallbackServerError(format!("Server error: {}", e)))
        });

        // Wait for callback with timeout
        let result = tokio::select! {
            callback_result = tokio::task::spawn_blocking(move || rx.recv()) => {
                match callback_result {
                    Ok(Ok(data)) => Ok(data),
                    Ok(Err(_)) => Err(AuthError::CallbackServerError("Channel closed unexpectedly".to_string())),
                    Err(e) => Err(AuthError::CallbackServerError(format!("Task join error: {}", e))),
                }
            },
            _ = tokio::time::sleep(self.timeout) => {
                Err(AuthError::Timeout)
            }
        };

        // Abort the server
        server_handle.abort();

        result
    }

    /// Generate success HTML page to display to user
    fn generate_success_page(&self) -> String {
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Authorization Successful</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            margin: 0;
            padding: 0;
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
            color: white;
        }
        .container {
            text-align: center;
            background: rgba(255, 255, 255, 0.1);
            padding: 3rem;
            border-radius: 20px;
            backdrop-filter: blur(10px);
            border: 1px solid rgba(255, 255, 255, 0.2);
            box-shadow: 0 15px 35px rgba(0, 0, 0, 0.1);
        }
        .check-icon {
            font-size: 4rem;
            color: #4ade80;
            margin-bottom: 1rem;
        }
        h1 {
            margin: 0 0 1rem 0;
            font-size: 2rem;
            font-weight: 300;
        }
        p {
            margin: 0;
            font-size: 1.1rem;
            opacity: 0.9;
            line-height: 1.5;
        }
        .close-instruction {
            margin-top: 2rem;
            font-size: 0.9rem;
            opacity: 0.7;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="check-icon">✓</div>
        <h1>Authorization Successful!</h1>
        <p>You have successfully authorized the Basilica CLI.</p>
        <p class="close-instruction">You can now close this window and return to the CLI.</p>
    </div>
    <script>
        // Auto-close after 3 seconds
        setTimeout(function() {
            window.close();
        }, 3000);
    </script>
</body>
</html>
        "#
        .to_string()
    }

    /// Generate error HTML page to display to user
    fn generate_error_page(&self, error: &str) -> String {
        format!(
            r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Authorization Failed</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #f093fb 0%, #f5576c 100%);
            margin: 0;
            padding: 0;
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
            color: white;
        }}
        .container {{
            text-align: center;
            background: rgba(255, 255, 255, 0.1);
            padding: 3rem;
            border-radius: 20px;
            backdrop-filter: blur(10px);
            border: 1px solid rgba(255, 255, 255, 0.2);
            box-shadow: 0 15px 35px rgba(0, 0, 0, 0.1);
            max-width: 500px;
        }}
        .error-icon {{
            font-size: 4rem;
            color: #fca5a5;
            margin-bottom: 1rem;
        }}
        h1 {{
            margin: 0 0 1rem 0;
            font-size: 2rem;
            font-weight: 300;
        }}
        p {{
            margin: 0 0 1rem 0;
            font-size: 1.1rem;
            opacity: 0.9;
            line-height: 1.5;
        }}
        .error-details {{
            background: rgba(0, 0, 0, 0.2);
            padding: 1rem;
            border-radius: 10px;
            margin: 1.5rem 0;
            font-family: monospace;
            font-size: 0.9rem;
            word-break: break-word;
        }}
        .close-instruction {{
            margin-top: 2rem;
            font-size: 0.9rem;
            opacity: 0.7;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="error-icon">✗</div>
        <h1>Authorization Failed</h1>
        <p>There was an error during the authorization process.</p>
        <div class="error-details">{}</div>
        <p class="close-instruction">Please close this window and try again in the CLI.</p>
    </div>
    <script>
        // Auto-close after 5 seconds
        setTimeout(function() {{
            window.close();
        }}, 5000);
    </script>
</body>
</html>
        "#,
            error
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('&', "&amp;")
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

    let response_html = if let Some(error) = &params.error {
        // Generate error page
        let error_msg = params.error_description.as_deref().unwrap_or(error);
        CallbackServer::new(8080, Duration::from_secs(300)).generate_error_page(error_msg)
    } else if params.code.is_some() {
        // Validate state parameter
        let expected_state = {
            let state_guard = state.lock().unwrap();
            state_guard.expected_state.clone()
        };

        if let Some(received_state) = &params.state {
            if received_state != &expected_state {
                let error_msg = format!(
                    "State mismatch: expected {}, got {}",
                    expected_state, received_state
                );
                CallbackServer::new(8080, Duration::from_secs(300)).generate_error_page(&error_msg)
            } else {
                // Send the callback data through the channel
                if let Ok(state_guard) = state.lock() {
                    let _ = state_guard.sender.send(callback_data);
                }
                CallbackServer::new(8080, Duration::from_secs(300)).generate_success_page()
            }
        } else {
            CallbackServer::new(8080, Duration::from_secs(300))
                .generate_error_page("Missing state parameter")
        }
    } else {
        CallbackServer::new(8080, Duration::from_secs(300))
            .generate_error_page("Missing authorization code")
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(response_html),
    )
}
