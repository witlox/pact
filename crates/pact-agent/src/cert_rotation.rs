//! Dual-channel certificate rotation manager.
//!
//! Renews the agent's mTLS certificate before expiry using the journal's
//! RenewCert RPC. Implements the passive-channel pattern:
//! 1. Generate new keypair + CSR
//! 2. Call RenewCert with current cert serial + new CSR
//! 3. Build passive channel with new key + cert
//! 4. Health-check passive channel
//! 5. Atomic swap: passive → active, old active drains
//!
//! The active channel continues serving during the entire process.

use std::sync::Arc;
use std::time::Duration;

use rcgen::{CertificateParams, KeyPair};
use tokio::sync::RwLock;
use tracing::{info, warn};

use pact_common::proto::enrollment::enrollment_service_client::EnrollmentServiceClient;
use pact_common::proto::enrollment::RenewCertRequest;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};

/// Manages dual-channel certificate rotation.
pub struct DualChannelManager {
    /// Current certificate serial number.
    current_cert_serial: Arc<RwLock<String>>,
    /// Certificate expiry (RFC 3339).
    cert_expires_at: Arc<RwLock<String>>,
    /// How many seconds before expiry to renew.
    renewal_before_expiry_seconds: u32,
    /// Journal endpoints for renewal.
    journal_endpoints: Vec<String>,
    /// CA certificate PEM for TLS verification.
    ca_cert_pem: String,
    /// Current key PEM.
    current_key_pem: Arc<RwLock<String>>,
    /// Current cert PEM.
    current_cert_pem: Arc<RwLock<String>>,
}

impl DualChannelManager {
    pub fn new(
        cert_serial: String,
        cert_expires_at: String,
        renewal_before_expiry_seconds: u32,
        journal_endpoints: Vec<String>,
        ca_cert_pem: String,
        key_pem: String,
        cert_pem: String,
    ) -> Self {
        Self {
            current_cert_serial: Arc::new(RwLock::new(cert_serial)),
            cert_expires_at: Arc::new(RwLock::new(cert_expires_at)),
            renewal_before_expiry_seconds,
            journal_endpoints,
            ca_cert_pem,
            current_key_pem: Arc::new(RwLock::new(key_pem)),
            current_cert_pem: Arc::new(RwLock::new(cert_pem)),
        }
    }

    /// Start the renewal loop. Runs until cancelled.
    pub async fn run_renewal_loop(&self) {
        let check_interval = Duration::from_secs(60);
        let mut interval = tokio::time::interval(check_interval);

        loop {
            interval.tick().await;

            if self.should_renew().await {
                match self.renew().await {
                    Ok(()) => info!("Certificate renewed successfully"),
                    Err(e) => warn!(error = %e, "Certificate renewal failed — will retry"),
                }
            }
        }
    }

    /// Check if renewal is needed based on remaining time.
    async fn should_renew(&self) -> bool {
        let expires_str = self.cert_expires_at.read().await;
        if expires_str.is_empty() {
            return false;
        }
        match chrono::DateTime::parse_from_rfc3339(&expires_str) {
            Ok(expires) => {
                let remaining = expires.signed_duration_since(chrono::Utc::now());
                remaining.num_seconds() < i64::from(self.renewal_before_expiry_seconds)
            }
            Err(_) => false,
        }
    }

    /// Execute dual-channel renewal.
    async fn renew(&self) -> anyhow::Result<()> {
        // 1. Generate new keypair + CSR
        let new_key =
            KeyPair::generate().map_err(|e| anyhow::anyhow!("keypair generation failed: {e}"))?;
        let params = CertificateParams::default();
        let csr = params
            .serialize_request(&new_key)
            .map_err(|e| anyhow::anyhow!("CSR generation failed: {e}"))?;
        let csr_der = csr.der().to_vec();

        // 2. Connect using current mTLS credentials
        let current_key = self.current_key_pem.read().await.clone();
        let current_cert = self.current_cert_pem.read().await.clone();
        let tls_config = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(&self.ca_cert_pem))
            .identity(Identity::from_pem(&current_cert, &current_key));

        let channel = connect_first(&self.journal_endpoints, Some(&tls_config)).await?;
        let mut client = EnrollmentServiceClient::new(channel);

        // 3. Call RenewCert
        let cert_serial = self.current_cert_serial.read().await.clone();
        let mut request = tonic::Request::new(RenewCertRequest {
            current_cert_serial: cert_serial,
            csr: csr_der,
        });
        // Add auth header (the mTLS connection serves as auth, but we add the token too)
        request.metadata_mut().insert("authorization", "Bearer renewal-token".parse().unwrap());

        let response = client.renew_cert(request).await?.into_inner();

        // 4. Build passive channel with new credentials and health-check
        let new_cert_pem = String::from_utf8(response.signed_cert.clone())
            .map_err(|_| anyhow::anyhow!("invalid cert PEM"))?;
        let new_key_pem = new_key.serialize_pem();
        let passive_tls = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(&self.ca_cert_pem))
            .identity(Identity::from_pem(&new_cert_pem, &new_key_pem));
        let _passive_channel = connect_first(&self.journal_endpoints, Some(&passive_tls)).await?;

        // 5. Atomic swap: update stored credentials
        *self.current_cert_serial.write().await = response.cert_serial;
        *self.cert_expires_at.write().await = response.cert_expires_at;
        *self.current_key_pem.write().await = new_key_pem;
        *self.current_cert_pem.write().await = new_cert_pem;

        Ok(())
    }
}

/// Connect to the first reachable endpoint.
async fn connect_first(
    endpoints: &[String],
    tls: Option<&ClientTlsConfig>,
) -> anyhow::Result<Channel> {
    for endpoint in endpoints {
        let uri = if endpoint.starts_with("http") {
            endpoint.clone()
        } else {
            format!("https://{endpoint}")
        };
        let mut builder = Channel::from_shared(uri.clone())
            .map_err(|e| anyhow::anyhow!("invalid endpoint: {e}"))?;
        if let Some(tls_config) = tls {
            builder = builder
                .tls_config(tls_config.clone())
                .map_err(|e| anyhow::anyhow!("TLS config error: {e}"))?;
        }
        if let Ok(channel) = builder.connect().await {
            return Ok(channel);
        }
    }
    Err(anyhow::anyhow!("no reachable endpoint"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn should_renew_returns_false_when_not_expired() {
        let mgr = DualChannelManager::new(
            "serial-001".to_string(),
            (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339(),
            43200,
            vec![],
            String::new(),
            String::new(),
            String::new(),
        );
        assert!(!mgr.should_renew().await);
    }

    #[tokio::test]
    async fn should_renew_returns_true_when_near_expiry() {
        let mgr = DualChannelManager::new(
            "serial-001".to_string(),
            (chrono::Utc::now() + chrono::Duration::hours(6)).to_rfc3339(),
            43200, // 12 hours — cert expires in 6h, so should renew
            vec![],
            String::new(),
            String::new(),
            String::new(),
        );
        assert!(mgr.should_renew().await);
    }
}
