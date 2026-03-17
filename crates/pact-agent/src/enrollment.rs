//! Agent enrollment manager — handles first-boot enrollment with CSR.
//!
//! Reads hardware identity from SMBIOS/DMI (Linux) or uses mock data (macOS),
//! generates a keypair + CSR via `rcgen`, and calls the journal's Enroll RPC
//! over a server-TLS-only channel.

use rcgen::{CertificateParams, KeyPair};
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use tracing::{debug, info, warn};

use pact_common::config::EnrollmentConfig;
use pact_common::proto::enrollment::enrollment_service_client::EnrollmentServiceClient;
use pact_common::proto::enrollment::{EnrollRequest, HardwareIdentity as ProtoHardwareIdentity};

/// Result of a successful enrollment.
#[derive(Debug, Clone)]
pub struct EnrollmentResult {
    pub node_id: String,
    pub domain_id: String,
    pub vcluster_id: Option<String>,
    pub cert_pem: Vec<u8>,
    pub ca_chain_pem: Vec<u8>,
    pub cert_serial: String,
    pub cert_expires_at: String,
    pub key_pair_pem: String,
}

/// Enrollment manager for the agent.
pub struct EnrollmentManager {
    config: EnrollmentConfig,
}

impl EnrollmentManager {
    pub fn new(config: EnrollmentConfig) -> Self {
        Self { config }
    }

    /// Read hardware identity from the system.
    ///
    /// On Linux, reads from SMBIOS/DMI files in /sys/class/dmi/.
    /// On macOS (dev), returns mock values.
    pub fn read_hardware_identity(&self) -> anyhow::Result<ProtoHardwareIdentity> {
        #[cfg(target_os = "linux")]
        {
            let mac = read_primary_mac().unwrap_or_else(|_| "00:00:00:00:00:00".to_string());
            let bmc_serial = std::fs::read_to_string("/sys/class/dmi/id/board_serial")
                .unwrap_or_default()
                .trim()
                .to_string();
            Ok(ProtoHardwareIdentity {
                mac_address: mac,
                bmc_serial,
                extra: std::collections::HashMap::new(),
            })
        }

        #[cfg(not(target_os = "linux"))]
        {
            warn!("Non-Linux platform — using mock hardware identity for development");
            Ok(ProtoHardwareIdentity {
                mac_address: "de:ad:be:ef:00:01".to_string(),
                bmc_serial: "MOCK-SERIAL-001".to_string(),
                extra: std::collections::HashMap::new(),
            })
        }
    }

    /// Generate a keypair and CSR for enrollment.
    ///
    /// The private key stays in agent memory — only the CSR (public key) is sent.
    pub fn generate_keypair_and_csr(&self) -> anyhow::Result<(KeyPair, Vec<u8>)> {
        let key_pair =
            KeyPair::generate().map_err(|e| anyhow::anyhow!("keypair generation failed: {e}"))?;
        let params = CertificateParams::default();
        let csr = params
            .serialize_request(&key_pair)
            .map_err(|e| anyhow::anyhow!("CSR generation failed: {e}"))?;
        Ok((key_pair, csr.der().to_vec()))
    }

    /// Connect to journal enrollment endpoint (server-TLS-only, no client cert).
    pub async fn connect_enrollment(&self) -> anyhow::Result<EnrollmentServiceClient<Channel>> {
        let ca_pem = std::fs::read_to_string(&self.config.ca_cert).map_err(|e| {
            anyhow::anyhow!("cannot read CA cert {}: {e}", self.config.ca_cert.display())
        })?;

        let tls_config =
            ClientTlsConfig::new().ca_certificate(Certificate::from_pem(ca_pem));

        for endpoint in &self.config.journal_endpoints {
            let uri = if endpoint.starts_with("http") {
                endpoint.clone()
            } else {
                format!("https://{endpoint}")
            };
            debug!(endpoint = %uri, "Trying enrollment endpoint");
            match Channel::from_shared(uri.clone())
                .map_err(|e| anyhow::anyhow!("invalid endpoint: {e}"))?
                .tls_config(tls_config.clone())
                .map_err(|e| anyhow::anyhow!("TLS config error: {e}"))?
                .connect()
                .await
            {
                Ok(channel) => {
                    info!(endpoint = %uri, "Connected to enrollment service");
                    return Ok(EnrollmentServiceClient::new(channel));
                }
                Err(e) => {
                    warn!(endpoint = %uri, error = %e, "Enrollment endpoint unreachable");
                }
            }
        }
        Err(anyhow::anyhow!("no reachable enrollment endpoint"))
    }

    /// Execute the enrollment flow: read hw identity, generate CSR, call Enroll RPC.
    pub async fn enroll(&self) -> anyhow::Result<EnrollmentResult> {
        let hw_identity = self.read_hardware_identity()?;
        let (key_pair, csr_der) = self.generate_keypair_and_csr()?;
        let mut client = self.connect_enrollment().await?;

        info!(mac = %hw_identity.mac_address, "Enrolling agent with journal");

        let response = client
            .enroll(EnrollRequest {
                hardware_identity: Some(hw_identity),
                csr: csr_der,
            })
            .await?
            .into_inner();

        let vcluster_id = if response.vcluster_id.is_empty() {
            None
        } else {
            Some(response.vcluster_id)
        };

        info!(
            node_id = %response.node_id,
            domain_id = %response.domain_id,
            vcluster = vcluster_id.as_deref().unwrap_or("(none)"),
            cert_serial = %response.cert_serial,
            "Enrollment successful"
        );

        Ok(EnrollmentResult {
            node_id: response.node_id,
            domain_id: response.domain_id,
            vcluster_id,
            cert_pem: response.signed_cert,
            ca_chain_pem: response.ca_chain,
            cert_serial: response.cert_serial,
            cert_expires_at: response.cert_expires_at,
            key_pair_pem: key_pair.serialize_pem(),
        })
    }
}

/// Read the primary network interface MAC address on Linux.
#[cfg(target_os = "linux")]
fn read_primary_mac() -> anyhow::Result<String> {
    // Read from first non-loopback interface
    let interfaces = std::fs::read_dir("/sys/class/net")?;
    for entry in interfaces {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" {
            continue;
        }
        let mac_path = entry.path().join("address");
        if let Ok(mac) = std::fs::read_to_string(&mac_path) {
            let mac = mac.trim().to_string();
            if mac != "00:00:00:00:00:00" {
                return Ok(mac);
            }
        }
    }
    anyhow::bail!("no primary MAC address found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn generate_keypair_and_csr_succeeds() {
        let config = EnrollmentConfig {
            journal_endpoints: vec!["localhost:9443".to_string()],
            ca_cert: PathBuf::from("/tmp/nonexistent-ca.pem"),
            cert_dir: PathBuf::from("/tmp/certs"),
            renewal_before_expiry_seconds: 43200,
        };
        let mgr = EnrollmentManager::new(config);
        let (key_pair, csr_der) = mgr.generate_keypair_and_csr().unwrap();
        assert!(!key_pair.serialize_pem().is_empty());
        assert!(!csr_der.is_empty());
    }

    #[test]
    fn hardware_identity_readable() {
        let config = EnrollmentConfig {
            journal_endpoints: vec![],
            ca_cert: PathBuf::from("/tmp/ca.pem"),
            cert_dir: PathBuf::from("/tmp/certs"),
            renewal_before_expiry_seconds: 43200,
        };
        let mgr = EnrollmentManager::new(config);
        let hw = mgr.read_hardware_identity().unwrap();
        assert!(!hw.mac_address.is_empty());
    }
}
