# Security Roadmap for ndld

This document tracks security improvements for the ndld OAuth authentication server.

## Completed

### XSS Vulnerability in Error Pages (CRITICAL)
- **Status**: Fixed
- **Location**: `routes.rs` - `error_html()` function
- **Issue**: Error messages were embedded into HTML without escaping using raw `format!()`. This allowed potential XSS attacks if error strings contained malicious JavaScript.
- **Fix**: Converted `error_html()` to use maud templating, which auto-escapes all content.

### Rate Limiting (HIGH)
- **Status**: Fixed
- **Location**: `routes.rs` - `create_router()` function
- **Issue**: No rate limiting on auth endpoints allowed DoS via session exhaustion.
- **Fix**: Added `tower_governor` middleware with per-IP rate limiting:
  - `/auth/start`: 10 requests per minute (prevents session exhaustion)
  - `/auth/poll`: 60 requests per minute (allows normal polling behavior)

## Planned Improvements

### Medium Priority

#### Error Information Disclosure
- **Location**: `routes.rs:117-120`
- **Issue**: Token exchange errors from Threads API are exposed to users, potentially revealing API implementation details.
- **Recommendation**: Return generic error messages to users while keeping detailed errors server-side in logs.
- **Example**:
  ```rust
  // Current (reveals API errors)
  error_html(&e).into_response()

  // Improved (generic message)
  error_html("Authentication failed. Please try again.").into_response()
  ```

#### Redirect URI Validation
- **Location**: `auth.rs:95-97`
- **Issue**: Redirect URI is constructed from `NDLD_PUBLIC_URL` environment variable without validation. Misconfiguration could redirect OAuth flow to attacker-controlled domain.
- **Recommendation**: Add startup validation that `NDLD_PUBLIC_URL` matches expected patterns or is explicitly whitelisted.
- **Example**:
  ```rust
  fn validate_public_url(url: &str) -> Result<(), String> {
      let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
      if parsed.scheme() != "https" {
          return Err("NDLD_PUBLIC_URL must use HTTPS".to_string());
      }
      Ok(())
  }
  ```

### Low Priority

#### Version Disclosure
- **Location**: `routes.rs:152-158`
- **Issue**: `/health` endpoint exposes exact version and git hash, which could help attackers identify known vulnerabilities.
- **Options**:
  1. Remove version info from public `/health` endpoint
  2. Add a separate authenticated `/health/debug` endpoint with version info
  3. Accept as low risk (defense in depth consideration)

#### Security Headers
- **Issue**: HTTP responses don't include security headers.
- **Recommendation**: Add middleware to include headers:
  ```
  Strict-Transport-Security: max-age=31536000; includeSubDomains
  X-Content-Type-Options: nosniff
  X-Frame-Options: DENY
  Content-Security-Policy: default-src 'self'
  ```

#### Request ID Tracking
- **Issue**: No request IDs for correlating logs across requests.
- **Recommendation**: Add `tower-http`'s `RequestId` middleware for better debugging and incident response.

### CI/CD Improvements

#### Dependency Auditing
- **Issue**: No automated security scanning for dependencies.
- **Recommendation**: Add `cargo audit` to CI pipeline:
  ```yaml
  - name: Security audit
    run: cargo audit
  ```

#### Container Scanning
- **Issue**: No vulnerability scanning of container images.
- **Recommendation**: Add Trivy or similar scanner to CI:
  ```yaml
  - name: Container scan
    uses: aquasecurity/trivy-action@master
    with:
      image-ref: 'ndld:latest'
  ```

## Security Contact

Report security issues via GitHub Issues at https://github.com/pgray/ndl or privately via email if the issue is sensitive.
