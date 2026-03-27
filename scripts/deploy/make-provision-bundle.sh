#!/usr/bin/env bash
# make-provision-bundle.sh — Create a single tarball for node provisioning.
#
# Bundles release binaries, deploy scripts, systemd units, and CA certs
# into one file for easy distribution to nodes.
#
# Usage: ./make-provision-bundle.sh <release-dir> <output-path> [ca-dir]
#   release-dir: directory containing downloaded release tarballs
#   output-path: where to write the bundle (e.g., /tmp/pact-provision.tar.gz)
#   ca-dir:      optional CA directory to include (default: none)
#
# The bundle is extracted on target nodes with:
#   tar xzf pact-provision.tar.gz -C /tmp
# Then install scripts are at /tmp/scripts/deploy/

set -euo pipefail

RELEASE_DIR="${1:?Usage: make-provision-bundle.sh <release-dir> <output-path> [ca-dir]}"
OUTPUT="${2:?}"
CA_DIR="${3:-}"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Verify inputs
for f in "$RELEASE_DIR"/pact-platform-*.tar.gz; do
    [ -f "$f" ] || { echo "ERROR: No pact-platform-*.tar.gz in $RELEASE_DIR"; exit 1; }
    break
done

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Copy release tarballs
cp "$RELEASE_DIR"/pact-*.tar.gz "$TMPDIR/"

# Copy deploy scripts and systemd units
cp -r "$REPO_ROOT/scripts/deploy" "$TMPDIR/scripts-deploy"
cp -r "$REPO_ROOT/infra/systemd" "$TMPDIR/infra-systemd"

# Optionally include CA
if [ -n "$CA_DIR" ] && [ -d "$CA_DIR" ]; then
    cp -r "$CA_DIR" "$TMPDIR/ca"
fi

# Create bundle
tar czf "$OUTPUT" -C "$TMPDIR" .

echo "Provisioning bundle created: $OUTPUT ($(du -h "$OUTPUT" | cut -f1))"
echo "Contents:"
tar tzf "$OUTPUT" | head -20
