#!/usr/bin/env bash
# setup-ca.sh — Generate ephemeral CA + node certificates for mTLS.
#
# Reusable on-prem. No GCP-specific logic.
#
# Usage: ./setup-ca.sh <cert-dir> <node-name> [ca-dir]
#   cert-dir: where to write node cert/key (e.g., /etc/pact)
#   node-name: CN for the node certificate
#   ca-dir: where CA cert/key live (default: /etc/pact/ca)
#
# If CA doesn't exist, creates it. Then signs a node cert.

set -euo pipefail

CERT_DIR="${1:?Usage: setup-ca.sh <cert-dir> <node-name> [ca-dir]}"
NODE_NAME="${2:?Usage: setup-ca.sh <cert-dir> <node-name> [ca-dir]}"
CA_DIR="${3:-/etc/pact/ca}"
DAYS_CA=3650
DAYS_NODE=365

mkdir -p "$CERT_DIR" "$CA_DIR"

# Generate CA if it doesn't exist
if [ ! -f "$CA_DIR/ca.key" ]; then
    echo "Generating ephemeral CA..."
    openssl genrsa -out "$CA_DIR/ca.key" 4096
    openssl req -new -x509 -key "$CA_DIR/ca.key" \
        -out "$CA_DIR/ca.crt" \
        -days "$DAYS_CA" \
        -subj "/CN=pact-ephemeral-ca/O=pact"
    chmod 600 "$CA_DIR/ca.key"
    echo "CA created: $CA_DIR/ca.crt"
fi

# Generate node key + CSR
echo "Generating certificate for $NODE_NAME..."
openssl genrsa -out "$CERT_DIR/node.key" 2048
openssl req -new -key "$CERT_DIR/node.key" \
    -out "$CERT_DIR/node.csr" \
    -subj "/CN=$NODE_NAME/O=pact"

# Sign with CA
openssl x509 -req -in "$CERT_DIR/node.csr" \
    -CA "$CA_DIR/ca.crt" \
    -CAkey "$CA_DIR/ca.key" \
    -CAcreateserial \
    -out "$CERT_DIR/node.crt" \
    -days "$DAYS_NODE" \
    -sha256

# Copy CA cert for verification
cp "$CA_DIR/ca.crt" "$CERT_DIR/ca.crt"

# Cleanup CSR
rm -f "$CERT_DIR/node.csr"

chmod 600 "$CERT_DIR/node.key"
chmod 644 "$CERT_DIR/node.crt" "$CERT_DIR/ca.crt"

echo "Certificates written to $CERT_DIR:"
echo "  node.crt  node.key  ca.crt"
