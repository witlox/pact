//! Dex container for lightweight OIDC authentication testing.
//!
//! Starts Dex with a static config containing a test client and static
//! password user. Starts in ~2-3 seconds vs Keycloak's ~60-180 seconds.

use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::Image;

/// Dex HTTP port.
pub const DEX_PORT: ContainerPort = ContainerPort::Tcp(5556);

/// Dex container image for lightweight OIDC testing.
#[derive(Debug, Clone)]
pub struct Dex {
    tag: String,
}

impl Default for Dex {
    fn default() -> Self {
        Self { tag: "v2.41.1".into() }
    }
}

impl Image for Dex {
    fn name(&self) -> &'static str {
        "ghcr.io/dexidp/dex"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        // Dex logs to stderr in some configurations, stdout in others.
        // Use StdErrOrOut to catch both.
        vec![WaitFor::message_on_stderr("listening on")]
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[DEX_PORT]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<std::borrow::Cow<'_, str>>> {
        ["dex", "serve", "/etc/dex/config.yaml"]
    }
}

/// Generate the Dex config YAML for testing.
/// The issuer_url must match what the container will be reachable at.
pub fn dex_test_config(issuer_url: &str) -> String {
    format!(
        r#"issuer: {issuer_url}

storage:
  type: sqlite3
  config:
    file: /tmp/dex.db

web:
  http: 0.0.0.0:5556

oauth2:
  skipApprovalScreen: true
  responseTypes: ["code", "token", "id_token"]
  grantTypes: ["authorization_code", "urn:ietf:params:oauth:grant-type:device_code", "password", "refresh_token"]
  passwordConnector: local

staticClients:
  - id: pact-cli
    name: "Pact CLI"
    secret: pact-test-secret
    redirectURIs:
      - "http://localhost:8085/callback"
      - "urn:ietf:wg:oauth:2.0:oob"

enablePasswordDB: true

staticPasswords:
  - email: "admin@pact.test"
    hash: "$2a$10$2b2cU8CPhOTaGrs1HRQuAueS7JTT5ZHsHSzYiFPm1leZck7Mc8T4W"
    username: "admin"
    userID: "08a8684b-db88-4b73-90a9-3cd1661f5466"
  - email: "ops@pact.test"
    hash: "$2a$10$2b2cU8CPhOTaGrs1HRQuAueS7JTT5ZHsHSzYiFPm1leZck7Mc8T4W"
    username: "ops"
    userID: "18a8684b-db88-4b73-90a9-3cd1661f5467"
"#
    )
}
