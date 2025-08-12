# OAuth Implementation Plan for Basilica

## Overview
This document tracks the implementation of OAuth2 authentication for Basilica using Auth0 with two supported flows:
1. **Authorization Code Flow + PKCE** (default) - for desktop environments
2. **Device Authorization Flow** (fallback) - for WSL, SSH, containers

## Implementation Status Tracker

### Phase 1: Core OAuth Foundation (Day 1)
- [ ] **Task 1.1**: Add OAuth Dependencies to `basilica-cli/Cargo.toml`
  - [ ] oauth2 = "4.4"
  - [ ] openidconnect = "3.4"
  - [ ] axum (workspace)
  - [ ] tower = "0.4"
  - [ ] tower-http = "0.4"
  - [ ] rand = "0.8"
  - [ ] base64 = "0.21"
  - [ ] sha2 = "0.10"
  - [ ] ring = "0.17"
  - [ ] keyring = "2.0"
  - [ ] directories = "5.0"
  - [ ] webbrowser = "0.8"
  - [ ] url = "2.4"

- [ ] **Task 1.2**: Create Auth Module Structure
  - [ ] Create `crates/basilica-cli/src/auth/` directory
  - [ ] Create `auth/mod.rs` - Module exports and public API
  - [ ] Create `auth/oauth_flow.rs` - PKCE OAuth implementation
  - [ ] Create `auth/device_flow.rs` - Device authorization implementation
  - [ ] Create `auth/callback_server.rs` - Local HTTP callback server
  - [ ] Create `auth/token_store.rs` - Secure token storage
  - [ ] Create `auth/types.rs` - Auth-related types (TokenSet, AuthConfig, etc.)

- [ ] **Task 1.3**: Implement PKCE Generation (`auth/oauth_flow.rs`)
  ```rust
  // Key functions to implement:
  - generate_pkce_verifier() -> String
  - generate_pkce_challenge(verifier: &str) -> String
  - generate_state() -> String
  - build_auth_url(config: &AuthConfig, challenge: &str, state: &str) -> Url
  ```

- [ ] **Task 1.4**: OAuth Client Setup (`auth/oauth_flow.rs`)
  ```rust
  // Configure OAuth2 client without client_secret:
  - create_oauth_client(config: &AuthConfig) -> BasicClient
  - NO client_secret for public client
  - Set Auth0 endpoints (authorize, token)
  - Configure redirect URI
  ```

- [ ] **Task 1.5**: Callback Server Implementation (`auth/callback_server.rs`)
  ```rust
  // Local HTTP server for OAuth callback:
  - start_callback_server() -> (ServerHandle, Receiver<CallbackResult>)
  - Handle GET /callback with code and state
  - Display success/error page to user
  - Graceful shutdown after receiving code
  - CORS headers for browser compatibility
  ```

### Phase 2: Token Management (Day 2)

- [ ] **Task 2.1**: Secure Token Storage (`auth/token_store.rs`)
  ```rust
  // OS keychain integration:
  - store_tokens(tokens: &TokenSet) -> Result<()>
  - get_tokens() -> Result<Option<TokenSet>>
  - delete_tokens() -> Result<()>
  - store_metadata(metadata: &TokenMetadata) -> Result<()>
  ```

- [ ] **Task 2.2**: Token Refresh Logic (`auth/token_store.rs`)
  ```rust
  // Automatic refresh implementation:
  - is_token_expired(token: &TokenSet) -> bool
  - needs_refresh(token: &TokenSet) -> bool // 5 min buffer
  - refresh_token(refresh_token: &str) -> Result<TokenSet>
  - handle_token_rotation(old: &TokenSet, new: &TokenSet)
  ```

- [ ] **Task 2.3**: Device Flow Implementation (`auth/device_flow.rs`)
  ```rust
  // Device authorization flow:
  - request_device_code(config: &AuthConfig) -> DeviceCodeResponse
  - display_user_code(code: &str, url: &str)
  - poll_for_token(device_code: &str) -> Result<TokenSet>
  - handle_poll_errors(error: &str) -> PollAction
  ```

- [ ] **Task 2.4**: CLI Commands (`cli/commands.rs` and handlers)
  - [ ] Add `Login` command with `--device-code` flag
  - [ ] Add `Logout` command
  - [ ] Add `Auth` command for status
  - [ ] Create `cli/handlers/auth.rs` for command handlers

### Phase 3: API JWT Validation (Day 3)

- [ ] **Task 3.1**: Add JWKS Dependencies to `basilica-api/Cargo.toml`
  ```toml
  jwks-client = "0.5"  # Already present, verify version
  ```

- [ ] **Task 3.2**: JWT Validation Implementation
  - [ ] Create `api/auth/jwt_validator.rs`
  ```rust
  // JWKS-based validation:
  - fetch_jwks(auth0_domain: &str) -> Result<JwkSet>
  - validate_jwt(token: &str, jwks: &JwkSet) -> Result<Claims>
  - verify_audience(claims: &Claims, expected: &str) -> Result<()>
  - verify_issuer(claims: &Claims, expected: &str) -> Result<()>
  ```

- [ ] **Task 3.3**: Authentication Middleware
  - [ ] Create `api/middleware/auth.rs`
  ```rust
  // JWT authentication middleware:
  - extract_bearer_token(headers: &HeaderMap) -> Option<String>
  - validate_request(token: &str) -> Result<UserClaims>
  - auth_middleware(req: Request, next: Next) -> Response
  - optional_auth_middleware(req: Request, next: Next) -> Response
  ```

- [ ] **Task 3.4**: Update Auth Config (`config/auth.rs`)
  ```rust
  // Add Auth0 configuration:
  - auth0_domain: String
  - auth0_client_id: String  
  - auth0_audience: String
  - jwks_cache_ttl: Duration
  - allowed_clock_skew: Duration
  ```

### Phase 4: Client Integration (Day 4)

- [ ] **Task 4.1**: Update BasilicaClient (`basilica-cli/src/client.rs` or similar)
  ```rust
  // JWT token integration:
  - load_auth_token() -> Option<String>
  - apply_bearer_auth(request: RequestBuilder) -> RequestBuilder
  - handle_401_response() -> Result<RetryAction>
  ```

- [ ] **Task 4.2**: Automatic Token Refresh in Client
  ```rust
  // Refresh on 401:
  - check_and_refresh_token() -> Result<()>
  - retry_with_new_token(request: Request) -> Result<Response>
  ```

- [ ] **Task 4.3**: Dual Authentication Support
  ```rust
  // Support both JWT and API keys:
  - try_jwt_auth() -> Option<String>
  - fallback_to_api_key() -> Option<String>
  - show_deprecation_notice()
  ```

### Phase 5: Polish & Testing (Day 5)

- [ ] **Task 5.1**: Error Handling
  - [ ] Network retry logic with exponential backoff
  - [ ] Auth0 error message translation
  - [ ] Timeout handling for all operations
  - [ ] Browser fallback when can't open

- [ ] **Task 5.2**: Environment Detection
  ```rust
  // Auto-detect environment:
  - is_wsl_environment() -> bool
  - is_ssh_session() -> bool  
  - is_container_runtime() -> bool
  - should_use_device_flow() -> bool
  ```

- [ ] **Task 5.3**: Create Auth Configuration File
  - [ ] Create `config/auth.toml`
  ```toml
  [auth0]
  domain = "basilica.auth0.com"
  client_id = ""  # Set via BASILICA_AUTH0_CLIENT_ID

  [auth0.advanced]
  token_refresh_margin_seconds = 300
  max_retry_attempts = 3
  callback_timeout_seconds = 300
  allowed_clock_skew_seconds = 60
  ```

- [ ] **Task 5.4**: Integration Tests
  - [ ] Test PKCE generation and validation
  - [ ] Test device flow with mock Auth0
  - [ ] Test token refresh scenarios
  - [ ] Test error handling paths
  - [ ] Test environment detection

## File Changes Summary

### New Files to Create:
1. `crates/basilica-cli/src/auth/mod.rs`
2. `crates/basilica-cli/src/auth/oauth_flow.rs`
3. `crates/basilica-cli/src/auth/device_flow.rs`
4. `crates/basilica-cli/src/auth/callback_server.rs`
5. `crates/basilica-cli/src/auth/token_store.rs`
6. `crates/basilica-cli/src/auth/types.rs`
7. `crates/basilica-cli/src/cli/handlers/auth.rs`
8. `crates/basilica-api/src/api/auth/jwt_validator.rs`
9. `crates/basilica-api/src/api/middleware/auth.rs`
10. `config/auth.toml`
11. `crates/basilica-cli/tests/auth_test.rs`
12. `crates/basilica-api/tests/jwt_test.rs`

### Files to Modify:
1. `crates/basilica-cli/Cargo.toml` - Add OAuth dependencies
2. `crates/basilica-cli/src/cli/commands.rs` - Add auth commands
3. `crates/basilica-cli/src/cli/handlers/mod.rs` - Export auth handler
4. `crates/basilica-cli/src/lib.rs` - Export auth module
5. `crates/basilica-cli/src/config/mod.rs` - Add auth config fields
6. `crates/basilica-api/src/config/auth.rs` - Add Auth0 config
7. `crates/basilica-api/src/api/middleware/mod.rs` - Add auth middleware
8. `crates/basilica-api/src/client.rs` - Support Bearer token auth
9. `crates/basilica-cli/src/main.rs` - Handle auth commands

## Implementation Notes

### Critical Requirements:
1. ❗ **NO client_secret** in PKCE flows - this is a public client
2. ❗ **RS256 only** - Auth0 requires RS256 for JWT signing
3. ❗ **OIDC Conformant** must be enabled in Auth0 settings
4. ❗ **Refresh Token Rotation** must be configured in Auth0

### Security Checklist:
- [ ] PKCE verifier is cryptographically random (32 bytes)
- [ ] State parameter expires after 10 minutes
- [ ] Tokens stored in OS keychain, not plain files
- [ ] Refresh tokens rotate on use
- [ ] JWKS cached with appropriate TTL
- [ ] Clock skew tolerance configured (60 seconds)
- [ ] All Auth0 communication over HTTPS

### User Experience Goals:
- [ ] Browser opens automatically for login
- [ ] Clear 6-digit code display for device flow
- [ ] Progress indicators during authentication
- [ ] Helpful error messages with recovery steps
- [ ] Automatic environment detection
- [ ] Seamless token refresh (no user interaction)

## Testing Checklist

### Unit Tests:
- [ ] PKCE verifier generation (32 bytes, base64url)
- [ ] PKCE challenge generation (SHA256)
- [ ] State parameter generation and validation
- [ ] Token expiry checking
- [ ] Token refresh timing (5-minute buffer)
- [ ] JWT claims validation
- [ ] JWKS parsing

### Integration Tests:
- [ ] Full PKCE flow with mock Auth0
- [ ] Device flow with mock endpoints
- [ ] Token refresh with rotation
- [ ] 401 retry logic
- [ ] Concurrent token refresh handling
- [ ] API key fallback

### Manual Testing:
- [ ] Desktop login flow
- [ ] WSL login with device code
- [ ] SSH session login
- [ ] Container environment login
- [ ] Token expiry and refresh
- [ ] Logout and re-login
- [ ] Network interruption handling

## Progress Tracking

### Day 1 Progress:
- [ ] All Phase 1 tasks completed
- [ ] Basic PKCE flow working
- [ ] Callback server functioning
- Notes: 

### Day 2 Progress:
- [ ] All Phase 2 tasks completed
- [ ] Token storage working
- [ ] Device flow implemented
- Notes:

### Day 3 Progress:
- [ ] All Phase 3 tasks completed
- [ ] JWT validation working
- [ ] API protected with auth
- Notes:

### Day 4 Progress:
- [ ] All Phase 4 tasks completed
- [ ] Client using JWT auth
- [ ] Dual auth support working
- Notes:

### Day 5 Progress:
- [ ] All Phase 5 tasks completed
- [ ] Tests passing
- [ ] Documentation updated
- Notes:

## Blockers and Issues
<!-- Track any blockers or issues encountered during implementation -->

## Decisions and Changes
<!-- Document any decisions made or changes from the original spec -->

## Next Steps After Implementation
1. Deploy Auth0 configuration to production
2. Update user documentation
3. Create migration guide for API key users
4. Monitor authentication metrics
5. Plan deprecation timeline for API keys