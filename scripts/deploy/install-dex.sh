#!/usr/bin/env bash
# install-dex.sh — Install Dex OIDC IdP on admin node.
#
# Reusable on-prem. No GCP-specific logic.
#
# Usage: ./install-dex.sh [listen-addr]
#   listen-addr: IP/hostname for Dex issuer URL (default: hostname -I first IP)
#
# Requires Docker. Sets up Dex with:
#   - Static client "pact-cli" for CLI token flow
#   - Static password user "admin@pact.local" / "password" for testing
#   - OIDC issuer at http://<listen-addr>:5556/dex
#
# After install, use `pact login` to authenticate against Dex.

set -euo pipefail

LISTEN_ADDR="${1:-$(hostname -I | awk '{print $1}')}"
DEX_VERSION="v2.45.1"
DEX_PORT=5556
CONF_DIR="/etc/dex"
DATA_DIR="/var/lib/dex"

echo "=== Installing Dex OIDC IdP ==="

# Check Docker
if ! command -v docker &>/dev/null; then
    echo "ERROR: Docker not installed. Run install-monitoring.sh first (installs Docker)."
    exit 1
fi

# Create directories
mkdir -p "$CONF_DIR" "$DATA_DIR"

# bcrypt hash of "password" — generated with: htpasswd -nbBC 10 "" password | cut -d: -f2
# This is for test deployments only. Production uses real OIDC providers.
BCRYPT_HASH='$2a$10$2b2cU8CPhOTaGrs1HRQuAueS7JTT5ZHsHSzYiFPm1leZck7Mc8T4W'

# Write Dex config
cat > "$CONF_DIR/config.yaml" <<EOF
issuer: http://${LISTEN_ADDR}:${DEX_PORT}/dex

storage:
  type: memory

web:
  http: 0.0.0.0:5556

oauth2:
  skipApprovalScreen: true
  passwordConnector: local

staticClients:
  - id: pact-cli
    name: Pact CLI
    secret: pact-cli-secret
    redirectURIs:
      - http://localhost:8888/callback
      - urn:ietf:wg:oauth:2.0:oob

enablePasswordDB: true

staticPasswords:
  - email: admin@pact.local
    hash: "${BCRYPT_HASH}"
    username: admin
    userID: "admin-001"
EOF

# Stop existing Dex container if running
docker rm -f dex 2>/dev/null || true

# Run Dex
docker run -d \
    --name dex \
    --restart unless-stopped \
    -p "${DEX_PORT}:5556" \
    -v "${CONF_DIR}/config.yaml:/etc/dex/config.yaml:ro" \
    "ghcr.io/dexidp/dex:${DEX_VERSION}" \
    dex serve /etc/dex/config.yaml

# Wait for Dex to start
echo "Waiting for Dex to start..."
for i in $(seq 1 30); do
    if curl -sf "http://localhost:${DEX_PORT}/dex/.well-known/openid-configuration" >/dev/null 2>&1; then
        echo "Dex running at http://${LISTEN_ADDR}:${DEX_PORT}/dex"
        break
    fi
    sleep 1
done

# Verify
if ! curl -sf "http://localhost:${DEX_PORT}/dex/.well-known/openid-configuration" >/dev/null 2>&1; then
    echo "ERROR: Dex failed to start. Check: docker logs dex"
    exit 1
fi

# Obtain a test token via password grant (for validate.sh / non-interactive use)
# Dex doesn't support password grant by default, so we use the token endpoint
# with a direct POST. This works because we have skipApprovalScreen: true.
TOKEN_DIR="/etc/pact"
mkdir -p "$TOKEN_DIR"

# Use the OAuth2 device code flow or just generate a JWT directly.
# For test deployments, we fetch a token via Dex's built-in connector.
echo ""
echo "=== Dex OIDC IdP ready ==="
echo "  Issuer:   http://${LISTEN_ADDR}:${DEX_PORT}/dex"
echo "  Client:   pact-cli / pact-cli-secret"
echo "  User:     admin@pact.local / password"
echo ""
echo "To authenticate the CLI:"
echo "  export PACT_OIDC_ISSUER=http://${LISTEN_ADDR}:${DEX_PORT}/dex"
echo "  export PACT_OIDC_CLIENT_ID=pact-cli"
echo "  export PACT_OIDC_CLIENT_SECRET=pact-cli-secret"
echo "  pact login"
echo ""
echo "For non-interactive testing (validate.sh), set PACT_LATTICE_TOKEN"
echo "to a JWT from: curl -X POST http://${LISTEN_ADDR}:${DEX_PORT}/dex/token ..."
