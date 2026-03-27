#!/bin/bash
# Management node startup — runs install-management.sh with node-specific params.
set -euo pipefail

# Deploy scripts installed to /opt/pact/deploy/ by Packer
/opt/pact/deploy/install-management.sh \
    "${node_id}" \
    "$(hostname -I | awk '{print $1}')" \
    "${peer_list}" \
    %{ if with_lattice }--with-lattice%{ endif }
