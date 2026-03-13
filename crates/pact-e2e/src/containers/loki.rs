//! Loki container for verifying event streaming pipeline.
//!
//! Starts Loki with a minimal in-memory config. Tests can push log entries
//! via the Loki HTTP API and query them back to verify the event pipeline.

use testcontainers::core::wait::HttpWaitStrategy;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::Image;

/// Loki HTTP API port.
pub const LOKI_HTTP_PORT: ContainerPort = ContainerPort::Tcp(3100);

/// Loki container image.
#[derive(Debug, Clone)]
pub struct Loki {
    tag: String,
}

impl Loki {
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Default for Loki {
    fn default() -> Self {
        Self { tag: "3.4.2".into() }
    }
}

impl Image for Loki {
    fn name(&self) -> &'static str {
        "grafana/loki"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![HttpWaitStrategy::new("/ready").with_expected_status_code(200u16).into()]
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[LOKI_HTTP_PORT]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<std::borrow::Cow<'_, str>>> {
        ["-config.file=/etc/loki/local-config.yaml"]
    }
}
