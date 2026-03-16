# hpc-auth Data Models

## Token Cache File Format

Path: `~/.config/{app}/tokens.json` (pact or lattice)

```json
{
  "version": 1,
  "default_server": "https://journal.example.com:9443",
  "servers": {
    "https://journal.example.com:9443": {
      "access_token": "eyJ...",
      "refresh_token": "dGhp...",
      "expires_at": "2026-03-14T18:30:00Z",
      "scopes": ["pact:admin"],
      "idp_issuer": "https://keycloak.example.com/realms/hpc"
    },
    "https://journal-staging.example.com:9443": {
      "access_token": "eyJ...",
      "refresh_token": null,
      "expires_at": "2026-03-14T19:00:00Z",
      "scopes": ["pact:admin"],
      "idp_issuer": "https://keycloak-staging.example.com/realms/hpc"
    }
  }
}
```

### Constraints
- File permissions: 0600 (enforced on read in strict mode, on write always)
- `refresh_token` is never logged (Auth7)
- `version` field for future schema evolution
- `default_server` is the server used when `--server` is not specified

## OIDC Discovery Cache

Path: `~/.config/{app}/discovery/{issuer_hash}.json`

```json
{
  "fetched_at": "2026-03-14T17:00:00Z",
  "ttl_seconds": 3600,
  "document": {
    "issuer": "https://keycloak.example.com/realms/hpc",
    "authorization_endpoint": "https://keycloak.example.com/realms/hpc/protocol/openid-connect/auth",
    "token_endpoint": "https://keycloak.example.com/realms/hpc/protocol/openid-connect/token",
    "revocation_endpoint": "https://keycloak.example.com/realms/hpc/protocol/openid-connect/revoke",
    "device_authorization_endpoint": "https://keycloak.example.com/realms/hpc/protocol/openid-connect/auth/device",
    "jwks_uri": "https://keycloak.example.com/realms/hpc/protocol/openid-connect/certs",
    "grant_types_supported": ["authorization_code", "urn:ietf:params:oauth:grant-type:device_code", "refresh_token"],
    "code_challenge_methods_supported": ["S256"]
  }
}
```

### Constraints
- `issuer_hash` is a stable hash of the issuer URL (no path traversal)
- Stale documents cleared on auth failure (F17)
- On fetch failure, stale cached document returned (degraded mode)

## Auth Discovery Endpoint Response

Returned by pact-journal at `GET /auth/discovery`:

```json
{
  "idp_url": "https://keycloak.example.com/realms/hpc",
  "client_id": "pact-cli",
  "scopes": ["openid", "profile"]
}
```

### Constraints
- Public endpoint — no authentication required (PAuth3)
- Served on telemetry port (9091), not gRPC port (9443)

## CLI Config Extension

New fields in `~/.config/pact/cli.toml`:

```toml
# Existing fields...
endpoint = "https://journal.example.com:9443"

# New auth fields:
[auth]
# Override IdP URL (skips server discovery)
idp_url = ""
# Override client ID
client_id = ""
# Permission mode: "strict" (default for pact) or "lenient"
permission_mode = "strict"
# Timeout for auth operations (seconds)
timeout = 30
```
