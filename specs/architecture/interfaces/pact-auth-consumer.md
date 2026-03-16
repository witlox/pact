# Pact Auth Consumer Interface

How pact-cli and pact-agent consume the hpc-auth crate.

## pact-cli Consumer

### New CLI Commands

```rust
// Added to Commands enum in main.rs:
Login {
    /// Server URL (overrides default).
    #[arg(long)]
    server: Option<String>,
    /// Force device code flow.
    #[arg(long)]
    device_code: bool,
    /// Use service account (client credentials) flow.
    #[arg(long)]
    service_account: bool,
},
Logout,
```

### Integration Points

1. **Login**: Constructs `AuthClient` with `PermissionMode::Strict` (PAuth1), calls `login()`.
2. **Logout**: Calls `logout()` on the `AuthClient`.
3. **All authenticated commands**: Call `auth.get_token()` before gRPC, inject into metadata.
4. **Unauthenticated commands**: `version`, `--help`, `login`, `logout` — bypass auth.

### Auth Discovery Endpoint (PAuth3)

pact-journal exposes a public (unauthenticated) discovery endpoint:

```
GET /auth/discovery → { "idp_url": "...", "client_id": "..." }
```

This is an HTTP endpoint on the telemetry/health server (port 9091), not a gRPC RPC.
The hpc-auth crate calls this to auto-discover the IdP before login.

### Token Flow

```
pact login
  → AuthClient::new(server_url, Strict)
  → auth.login()
    → fetch journal:9091/auth/discovery → {idp_url, client_id}
    → fetch idp/.well-known/openid-configuration → OidcDiscovery
    → select flow (PKCE > Confidential > DeviceCode > ManualPaste)
    → execute flow → TokenSet
    → cache.write(server_url, tokens) with 0600 perms

pact status (any authenticated command)
  → auth.get_token()
    → cache.read(server_url) → TokenSet
    → if expired: refresh silently
    → return access_token
  → inject "Authorization: Bearer {token}" into gRPC metadata
  → proceed with RPC
```

## pact-agent Consumer

The agent uses mTLS (machine identity), not OAuth2. However, the shell server
validates incoming OIDC tokens from CLI users. This validation is already
implemented via `shell/auth.rs` (JWKS + HS256 fallback).

The agent does NOT use hpc-auth for its own authentication — it uses mTLS
certificates provisioned by OpenCHAMI (A-I2).

## Break-Glass Path (PAuth4)

When the IdP is down and tokens are expired:
1. `pact login` fails with `AuthError::IdpUnreachable`
2. Error message suggests: "Use BMC console for emergency access"
3. Admin accesses node via BMC/Redfish console (out-of-band, via OpenCHAMI)
4. BMC provides unrestricted bash — changes detected as unattributed drift
5. When pact-agent recovers, drift is reported and logged

## Consumer-Specific Behavior

| Behavior | PACT (pact-cli) | Lattice (lattice-cli) |
|----------|-----------------|----------------------|
| Permission mode | Strict (PAuth1) | Lenient |
| Cache rejected on wrong perms | Yes, error | No, warn + fix |
| Emergency bypass | Via BMC console (PAuth4) | N/A |
| Two-person approval | Validates distinct identities (PAuth5) | N/A |
| Default scopes | `pact:admin` | `lattice:user` |
| Break-glass | BMC console | N/A |
