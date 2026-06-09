---
name: auth-playbook
description: Authentication and authorization workflow patterns for backend services, covering JWT validation, OAuth2 token exchange, session lifecycle management, middleware integration, and refresh token rotation strategies.
tags:
- auth
- security
- jwt
- oauth2
- session
- token
- middleware
---

# auth-playbook

Authentication and authorization workflow patterns for backend services, covering JWT validation, OAuth2 token exchange, session lifecycle management, middleware integration, and refresh token rotation strategies.

## Procedures

### JWT Validation Pipeline
- Extract the `Authorization: Bearer <token>` header from incoming HTTP requests in a shared middleware layer.
- Decode the JWT without verification first to inspect the `kid` (key ID) header and route to the correct public key.
- Fetch the public key from JWKS endpoint or local key store, caching it for `cache_ttl_secs` to avoid per-request HTTP calls.
- Verify the JWT signature, expiration (`exp`), not-before (`nbf`), issuer (`iss`), and audience (`aud`) claims using the `jsonwebtoken` crate.
- Reject tokens with `exp` within a 30-second clock-skew buffer to handle distributed clock drift.
- On validation failure, return `401 Unauthorized` with a machine-readable error code: `token_expired`, `invalid_signature`, `bad_issuer`.
- If validation succeeds, extract claims into a typed struct and inject into request extensions for downstream handlers.

### OAuth2 Authorization Code Flow
- Redirect unauthenticated users to the authorization endpoint with `response_type=code`, `client_id`, `redirect_uri`, `scope`, and a cryptographically random `state` parameter.
- Persist the `state` parameter with a 10-minute TTL in Redis to prevent CSRF on the callback.
- On callback, validate that `state` matches the stored value, then exchange the authorization code for tokens at the token endpoint.
- Store the access token and refresh token encrypted at rest using `aes-gcm` with a key derived from a master secret.
- Use PKCE (`code_challenge` with S256) for public clients — never transmit the client secret to mobile or SPA applications.
- Rotate the refresh token on each use: issue a new refresh token and invalidate the previous one server-side.

### Session Lifecycle Management
- Create a session record with `session_id: Uuid`, `user_id`, `created_at`, `expires_at`, `scopes: Vec<String>`, and `metadata: Json`.
- Store session in Redis with TTL matching `expires_at - now` for automatic cleanup; also write to PostgreSQL for audit trails.
- On each authenticated request, extend the session TTL by `sliding_window_secs` to implement sliding expiration.
- Enforce absolute session expiry regardless of activity — sessions older than `max_session_age_secs` are rejected even if recently active.
- Provide a `/logout` endpoint that deletes the session from Redis and marks it as revoked in PostgreSQL.
- Implement session invalidation on password change or role revocation by scanning Redis for all sessions belonging to the affected user.

### Token Refresh Strategy
- Clients call `/auth/refresh` with the refresh token to obtain a new access token without re-authentication.
- Validate the refresh token against the database, checking that it has not been revoked and has not expired.
- If the refresh token is within `rotation_window_secs` of its TTL, rotate it proactively: issue a new refresh token alongside the new access token.
- Detect refresh token reuse: if a previously-used refresh token is presented, revoke the entire token family and force re-authentication.
- Bind refresh tokens to the client's `User-Agent` and IP range; reject if the binding changed since issuance.

### Middleware Integration
- Mount authentication middleware before authorization middleware in the tower/layer stack: `auth_layer(auth_config).then(authz_layer(policy_store))`.
- Populate request extensions with `AuthenticatedUser { user_id, roles, scopes, session_id }` for downstream handlers.
- Apply route-level guards via `#[require_role("admin")]` or `#[require_scope("write:skills")]` annotations that inspect extensions.
- Log every authentication decision at `info` level: `tracing::info!(user_id, outcome, latency_ms, "auth decision")`.
- Return structured error responses with `WWW-Authenticate` headers describing required authentication schemes.

### Rate Limiting and Abuse Prevention
- Apply token-bucket rate limiting on the `/auth/login` endpoint: 5 attempts per minute per IP, 10 per minute per account.
- On repeated failures, escalate to exponential backoff: lock the account for `2^attempts` seconds after 5 consecutive failures.
- Monitor for credential-stuffing patterns: high-volume login attempts with varied usernames from a single IP.
- Rate-limit token refresh separately: 30 refreshes per hour per session to prevent refresh-token looping attacks.
- Emit `auth.account_locked` and `auth.suspicious_activity` events to the event stream for security monitoring.

## Conventions

- Never log raw tokens, passwords, or secret keys — redact them in tracing spans with `tracing::field::display(Redacted(token))`.
- All cryptographic operations use constant-time comparison for token and secret verification.
- Token payloads are deserialized into strongly-typed structs with `#[serde(rename_all = "camelCase")]` — never access claims as raw `serde_json::Value`.
- Refresh tokens are stored as SHA-256 hashes in the database; the raw token is only known to the client.
- Authentication configuration is loaded from environment variables: `JWT_PUBLIC_KEY`, `OAUTH_CLIENT_ID`, `OAUTH_CLIENT_SECRET`, `SESSION_REDIS_URL`.
- Use `ring` or `aws-lc-rs` for cryptographic primitives — never implement your own crypto.
- JWKS endpoints are fetched with a timeout of 3 seconds and a circuit breaker that opens after 5 consecutive failures.
- Access tokens have a short lifetime (5-15 minutes); refresh tokens last 7-30 days depending on sensitivity.

## Assets

```rust
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
    iat: usize,
    iss: String,
    aud: String,
    scope: String,
}

async fn validate_bearer_token(
    token: &str,
    decoding_key: &DecodingKey,
) -> Result<Claims, AuthError> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&["https://auth.example.com"]);
    validation.set_audience(&["skill-layer-api"]);
    validation.validate_exp = true;
    validation.leeway = 30;

    let token_data = decode::<Claims>(token, decoding_key, &validation)
        .map_err(|e| AuthError::InvalidToken(e.to_string()))?;

    Ok(token_data.claims)
}

async fn rotate_refresh_token(
    old_token_hash: &str,
    user_id: Uuid,
    store: &RefreshTokenStore,
) -> Result<RefreshTokenPair, AuthError> {
    store.mark_used_and_revoke_family(old_token_hash).await?;
    let new_pair = store.issue_token_pair(user_id).await?;
    Ok(new_pair)
}
```

### Multi-Factor Authentication
- Support TOTP-based MFA with a setup flow: generate a shared secret, display a QR code, verify one code, then enable MFA.
- On login, after primary credential validation, prompt for TOTP code if MFA is enabled for the account.
- Implement backup codes as single-use recovery tokens: generate 10 random codes, store their bcrypt hashes, and invalidate each on use.
- Support WebAuthn/FIDO2 as a phishing-resistant factor for high-security deployments, using the `webauthn-rs` crate.
- MFA setup and enforcement are per-user configurable, stored as flags on the user profile in PostgreSQL.

### Session Termination
- Provide a `/sessions` endpoint listing all active sessions with device info, IP, and creation time.
- Allow users to revoke individual sessions or all sessions except the current one.
- Administrators can terminate any user's sessions via an admin API with audit trail recording.
- On session termination, emit `auth.session_revoked` events so real-time services can disconnect WebSocket connections.
- Implement an idle timeout: sessions with no activity for `idle_timeout_secs` are terminated server-side regardless of token expiry.