//! Keycloak container for real OAuth2/OIDC authentication testing.
//!
//! Starts Keycloak in development mode with a pre-configured realm
//! containing pact-specific clients, roles, and test users.

use testcontainers::core::wait::HttpWaitStrategy;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::Image;

/// Keycloak HTTP port.
pub const KEYCLOAK_PORT: ContainerPort = ContainerPort::Tcp(8080);

/// Keycloak container image for OIDC authentication.
#[derive(Debug, Clone)]
pub struct Keycloak {
    tag: String,
}

impl Keycloak {
    /// Create a new Keycloak image with the given tag.
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Default for Keycloak {
    fn default() -> Self {
        Self { tag: "26.2".into() }
    }
}

impl Image for Keycloak {
    fn name(&self) -> &'static str {
        "quay.io/keycloak/keycloak"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        // Keycloak dev mode serves on 8080 — wait for the health endpoint
        vec![HttpWaitStrategy::new("/health/ready").with_expected_status_code(200u16).into()]
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[KEYCLOAK_PORT]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<std::borrow::Cow<'_, str>>> {
        ["start-dev"]
    }

    fn env_vars(
        &self,
    ) -> impl IntoIterator<
        Item = (impl Into<std::borrow::Cow<'_, str>>, impl Into<std::borrow::Cow<'_, str>>),
    > {
        [
            ("KC_BOOTSTRAP_ADMIN_USERNAME", "admin"),
            ("KC_BOOTSTRAP_ADMIN_PASSWORD", "admin"),
            ("KC_HEALTH_ENABLED", "true"),
        ]
    }
}
