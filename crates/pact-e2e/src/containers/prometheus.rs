//! Prometheus container for verifying journal metrics scraping.
//!
//! Starts Prometheus with a minimal config that scrapes the pact-journal
//! telemetry endpoint. Tests can query the Prometheus HTTP API to verify
//! that expected metrics are collected.

use testcontainers::core::wait::HttpWaitStrategy;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::Image;

/// Prometheus container port (HTTP API).
pub const PROMETHEUS_PORT: ContainerPort = ContainerPort::Tcp(9090);

/// Prometheus container image.
#[derive(Debug, Clone)]
pub struct Prometheus {
    tag: String,
}

impl Prometheus {
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Default for Prometheus {
    fn default() -> Self {
        Self { tag: "v3.2.1".into() }
    }
}

impl Image for Prometheus {
    fn name(&self) -> &'static str {
        "prom/prometheus"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![HttpWaitStrategy::new("/-/ready").with_expected_status_code(200u16).into()]
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[PROMETHEUS_PORT]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<std::borrow::Cow<'_, str>>> {
        [
            "--config.file=/etc/prometheus/prometheus.yml",
            "--storage.tsdb.retention.time=1h",
            "--web.enable-lifecycle",
        ]
    }
}

/// Minimal Prometheus config that scrapes a local target.
pub fn scrape_config(target_addr: &str) -> String {
    format!(
        r#"global:
  scrape_interval: 5s
  evaluation_interval: 5s

scrape_configs:
  - job_name: "pact-journal"
    static_configs:
      - targets: ["{target_addr}"]
"#
    )
}
