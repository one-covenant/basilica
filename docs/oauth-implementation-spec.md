# OAuth Implementation Specification for Basilica

## Overview
This document outlines the implementation plan for adding OAuth2 authentication to Basilica using Auth0 with two supported flows:
1. **Authorization Code Flow + PKCE** (default) - for desktop environments
2. **Device Authorization Flow** (fallback) - for WSL, SSH, containers, or when localhost is unavailable

## Architecture Overview

### Primary Flow: Authorization Code + PKCE
When users run `basilica login`, the CLI will:
1. Start a local HTTP server on localhost:8080
2. Open browser with Auth0 login page
3. After login, Auth0 redirects back to localhost:8080 with authorization code
4. CLI exchanges code for tokens using PKCE
5. Stores tokens locally for future API calls

### Alternative Flow: Device Authorization (6-digit code)
When user runs `basilica login --device-code`:
1. CLI requests device code from Auth0
2. Auth0 returns a user code (6-8 characters) and verification URL
3. CLI displays the code and URL to user
4. User visits URL on any device and enters the code
5. CLI polls Auth0 for tokens until authentication completes
6. Stores tokens locally for future API calls

## 🔴 Critical Implementation Notes

1. **NO Client Secret for PKCE**: The oauth2 crate BasicClient should NOT include client_secret for PKCE flows
2. **Auth0 requires RS256**: Must configure "JsonWebToken Signature Algorithm" to RS256 in Auth0 dashboard
3. **OIDC Conformant**: Must enable "OIDC Conformant" in Auth0 application settings
4. **Refresh Token Rotation**: Auth0 requires explicit configuration for refresh token rotation

## 1. Auth0 Configuration

### Native Application Settings
```
Type: Native
Grant Types: 
  - Authorization Code (for PKCE flow)
  - Device Code (for device authorization flow)
Token Endpoint Authentication Method: None
Allowed Callback URLs: http://localhost:8080/callback
Allowed Logout URLs: http://localhost:8080/logout
Refresh Token Rotation: Enabled
Refresh Token Expiration: 
  - Absolute: 30 days
  - Inactivity: 7 days

Advanced Settings:
  - Enable Device Code Grant Type
  - Device Code Expiration: 600 seconds (10 minutes)
  - Polling Interval: 5 seconds
```

### API Settings
```
Identifier: https://api.basilica.network
Signing Algorithm: RS256 (REQUIRED)
RBAC: Enable
Add Permissions in Access Token: Yes
```

## 2. Dependencies

### basilica-cli Dependencies
```toml
# crates/basilica-cli/Cargo.toml
[dependencies]
# Core OAuth
oauth2 = "4.4"
openidconnect = "3.4"  # Better for Auth0 - built on oauth2

# Local server
axum = { workspace = true }
tower = { version = "0.4", features = ["util"] }
tower-http = { version = "0.4", features = ["cors"] }

# Security
rand = "0.8"
base64 = "0.21"
sha2 = "0.10"
ring = "0.17"  # Better crypto performance

# Token storage
keyring = "2.0"  # Cross-platform secure storage
directories = "5.0"  # Platform-specific directories

# Utilities
webbrowser = "0.8"
url = "2.4"
chrono = { workspace = true }
```

### basilica-api Dependencies
```toml
# crates/basilica-api/Cargo.toml
[dependencies]
# Use jsonwebtoken instead of alcoholic_jwt for better performance
jsonwebtoken = "9.2"
jwks-client = "0.5"  # Auto-fetches and caches JWKS
```

## 3. CLI Implementation

### New Commands
```rust
// Add to Commands enum in cli/commands.rs
/// Authenticate with Basilica
Login {
    /// Use device code flow (for WSL, SSH, containers)
    #[arg(long)]
    device_code: bool,
},

/// Log out from Basilica
Logout,

/// Show authentication status
Auth,
```

### Module Structure
```
crates/basilica-cli/src/auth/
├── mod.rs              # Main auth module
├── oauth_flow.rs       # OAuth2 + PKCE implementation
├── device_flow.rs      # Device authorization flow implementation
├── callback_server.rs  # Local HTTP server for callback
├── token_store.rs      # Secure token storage
└── types.rs           # Auth-related types
```

### Key Implementation Requirements

#### PKCE Generation
- Generate cryptographically secure random 32-byte verifier
- Create challenge using SHA256 hash of verifier
- Use base64url encoding without padding

#### OAuth Client Setup
- Configure client without client_secret (public client)
- Set Auth0 domain endpoints for authorize and token
- Configure redirect URI for localhost callback
- Support both PKCE and device code grant types

#### Token Storage Strategy
- Use OS keychain/keyring for secure token storage
- Store access and refresh tokens separately
- Store metadata (expiry, user info) in platform config directory
- Implement automatic token refresh before expiry (5-minute buffer)
- Support token rotation on refresh

#### Callback Server Requirements
- Start HTTP server on localhost:8080 (fallback to random port if occupied)
- Handle OAuth callback at `/callback` endpoint
- Extract authorization code and state from query parameters
- Implement graceful shutdown after receiving code
- Include CORS headers for browser compatibility
- Display success/error page to user after callback

### Device Authorization Flow

#### Flow Steps
1. **Request Device Code**
   - POST to `https://{domain}/oauth/device/code`
   - Include client_id and scope
   - Receive device_code, user_code, verification_uri

2. **Display Instructions**
   - Show verification URL to user
   - Display user code (formatted for readability, e.g., "ABCD-1234")
   - Provide clear instructions

3. **Poll for Token**
   - POST to `https://{domain}/oauth/token`
   - Include device_code and grant_type="urn:ietf:params:oauth:grant-type:device_code"
   - Poll at specified interval (default 5 seconds)
   - Handle responses:
     - `authorization_pending`: Continue polling
     - `slow_down`: Increase polling interval
     - `access_denied`: User denied access
     - `expired_token`: Device code expired
     - Success: Receive access and refresh tokens

4. **Token Storage**
   - Use same secure storage as PKCE flow
   - Store refresh token for future use

#### When to Use Device Code Flow
Users should specify `--device-code` flag when:
- Running in WSL where localhost isn't accessible from Windows browser
- Running in SSH sessions
- Running in containers
- Any environment where localhost callback won't work

#### User Experience
```
$ basilica login --device-code
Please visit: https://basilica.auth0.com/activate
And enter code: ABCD-1234

Waiting for authentication...
✓ Successfully authenticated!
✓ Credentials saved securely
```

#### Error Scenarios
- Code expiration: Prompt user to restart login
- Network failures: Retry with exponential backoff
- User denial: Clear message about access denial
- Rate limiting: Respect `slow_down` response

## 4. API Implementation

### JWT Validation Requirements
- Fetch and cache JWKS from Auth0's well-known endpoint
- Validate RS256 signed tokens
- Verify audience matches API identifier
- Verify issuer matches Auth0 domain
- Check token expiration and not-before claims
- Auto-refresh JWKS cache periodically

### Authentication Middleware Requirements
- Extract Bearer token from Authorization header
- Validate JWT using cached JWKS
- Add user claims to request context
- Return 401 for invalid/expired tokens
- Support optional authentication for public endpoints

## 5. Integration with Existing Code

### BasilicaClient Integration
- The existing client already supports Bearer token authentication
- Modify client initialization to load JWT from token store
- Implement automatic token refresh on 401 responses
- Support both JWT and legacy API key authentication

## 6. Security Features

- **PKCE**: Prevents authorization code interception
- **State parameter**: CSRF protection with expiration
- **Secure storage**: OS keychain for tokens
- **JWT validation**: RS256 with JWKS
- **Automatic refresh**: Before token expiry (5 min margin)
- **HTTPS only**: All Auth0 communication
- **Token rotation**: Refresh tokens rotate on use

## 7. User Experience

### Standard Login Flow
```bash
$ basilica login
Opening browser for authentication...
Waiting for authentication...
✓ Successfully authenticated!
✓ Credentials saved securely
```

### Device Code Flow
```bash
$ basilica login --device-code
Please visit: https://basilica.auth0.com/activate
And enter code: ABCD-1234

Waiting for authentication...
✓ Successfully authenticated!
✓ Credentials saved securely
```

### Logout
```bash
$ basilica logout
✓ Successfully logged out
✓ Credentials removed
```

### Error Handling
- Graceful fallback when browser cannot open
- Clear timeout messages with retry instructions  
- Network error handling with offline detection
- Auth0-specific error message translation
- WSL/container detection with appropriate guidance

## 8. Configuration

```toml
# config/auth.toml (separate from main config)
[auth0]
domain = "basilica.auth0.com"  # Can be overridden by env
client_id = ""  # Set via BASILICA_AUTH0_CLIENT_ID env var

[auth0.advanced]
token_refresh_margin_seconds = 300  # Refresh 5 min before expiry
max_retry_attempts = 3
callback_timeout_seconds = 300
allowed_clock_skew_seconds = 60  # For JWT validation
```

## 9. Migration Support

### Dual Authentication Support
- Check for OAuth tokens first
- Fall back to legacy API keys if no tokens found
- Display deprecation notice when using legacy keys
- Provide clear migration instructions to users
- Support both authentication methods during transition period

## 10. Implementation Timeline

1. **Phase 1** (Day 1): Basic OAuth flow with PKCE
   - Set up Auth0 applications
   - Implement PKCE generation
   - Create callback server
   - Basic token exchange

2. **Phase 2** (Day 2): Token storage and refresh
   - Integrate keyring for secure storage
   - Implement refresh token logic
   - Add automatic refresh

3. **Phase 3** (Day 3): API-side validation
   - Set up JWKS client
   - Create JWT validation middleware
   - Protect routes

4. **Phase 4** (Day 4): Error handling and UX improvements
   - Better error messages
   - Progress indicators
   - Browser fallback

5. **Phase 5** (Day 5): Testing and migration support
   - Integration tests
   - Legacy API key support
   - Documentation

## 11. Testing Strategy

### Unit Tests
- PKCE generation and validation
- State parameter handling
- Token storage and retrieval

### Integration Tests
- Mock Auth0 endpoints for PKCE flow testing
- Mock device authorization endpoints
- Test token refresh logic
- Verify secure storage operations
- Test auto-detection logic for WSL/containers
- Validate error handling and retry logic

## 12. Important Notes

- Always use RS256 for JWT signing (Auth0 requirement)
- Never include client_secret in PKCE flows
- Refresh tokens should rotate on use
- JWKS should be cached with TTL
- State parameters should expire after 10 minutes
- Tokens should refresh 5 minutes before expiry