#!/bin/bash
# Admin node startup — installs monitoring stack.
set -euo pipefail

/opt/pact/deploy/install-monitoring.sh "${journal_hosts}"
