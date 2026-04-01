#!/bin/bash
# Management node startup — runs install-management.sh with node-specific params.
set -euo pipefail

# Export admin IP for OIDC (Dex runs on admin node)
export PACT_ADMIN_IP="${admin_ip}"

# Deploy scripts installed to /opt/pact/deploy/ by Packer
/opt/pact/deploy/install-management.sh \
    "${node_id}" \
    "$(hostname -I | awk '{print $1}')" \
    "${peer_list}" \
    %{ if node_id == 1 }--bootstrap%{ endif } \
    %{ if with_lattice }--with-lattice%{ endif }

# Append OIDC config to journal env (Dex runs on admin node)
echo "PACT_OIDC_ISSUER=http://${admin_ip}:5556/dex" >> /etc/pact/journal.env
echo "PACT_OIDC_AUDIENCE=pact-cli" >> /etc/pact/journal.env
echo "PACT_OIDC_JWKS_URL=http://${admin_ip}:5556/dex/keys" >> /etc/pact/journal.env
systemctl restart pact-journal
echo "OIDC configured for journal (issuer: http://${admin_ip}:5556/dex)"
