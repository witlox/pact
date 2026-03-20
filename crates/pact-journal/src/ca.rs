//! CA key manager — generates or loads CA key, signs enrollment certificates locally.
//!
//! Two modes:
//! - **Generate** (default): creates an ephemeral self-signed CA at journal startup.
//!   The CA key lives in memory. CA cert is distributed to agents via enrollment response.
//! - **Load**: reads a pre-provisioned CA key from PEM files on disk (legacy, for
//!   deployments that manage CA keys externally).
//!
//! No external CA dependency (no Vault). When SPIRE is deployed, this entire
//! module is only used as a fallback for non-SPIRE deployments.

use rcgen::{BasicConstraints, CertificateParams, CertifiedKey, DnType, IsCa, KeyPair};
use tracing::{debug, info};

/// Manages the CA key for signing enrollment certificates.
pub struct CaKeyManager {
    /// The CA certificate + key pair (self-signed or loaded).
    ca_certified_key: CertifiedKey,
    /// Certificate lifetime in seconds.
    cert_lifetime_seconds: u32,
}

impl CaKeyManager {
    /// Generate an ephemeral self-signed CA.
    ///
    /// The CA key exists only in memory. The CA cert is included in
    /// enrollment responses so agents can validate the trust chain.
    /// On journal restart, a new CA is generated (agents re-enroll
    /// on reconnect, so this is safe).
    ///
    /// For persistent CA across journal restarts, use `load()` with
    /// PEM files on disk, or deploy SPIRE as the primary identity provider.
    pub fn generate(domain_id: &str, cert_lifetime_seconds: u32) -> anyhow::Result<Self> {
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.distinguished_name.push(DnType::CommonName, format!("pact-ca-{domain_id}"));

        let ca_key =
            KeyPair::generate().map_err(|e| anyhow::anyhow!("CA key generation failed: {e}"))?;
        let ca_cert = ca_params
            .self_signed(&ca_key)
            .map_err(|e| anyhow::anyhow!("CA self-signing failed: {e}"))?;

        info!(domain_id, "generated ephemeral CA for enrollment signing");

        Ok(Self {
            ca_certified_key: CertifiedKey { cert: ca_cert, key_pair: ca_key },
            cert_lifetime_seconds,
        })
    }

    /// Load CA certificate and key from PEM files on disk.
    ///
    /// Used when CA keys are managed externally (e.g., provisioned by
    /// an operator or automation tool). Not required when using `generate()`.
    pub fn load(
        ca_cert_path: &std::path::Path,
        ca_key_path: &std::path::Path,
        cert_lifetime_seconds: u32,
    ) -> anyhow::Result<Self> {
        let ca_key_pem = std::fs::read_to_string(ca_key_path)
            .map_err(|e| anyhow::anyhow!("cannot read CA key {}: {e}", ca_key_path.display()))?;
        let _ca_cert_pem = std::fs::read_to_string(ca_cert_path)
            .map_err(|e| anyhow::anyhow!("cannot read CA cert {}: {e}", ca_cert_path.display()))?;

        let ca_key_pair =
            KeyPair::from_pem(&ca_key_pem).map_err(|e| anyhow::anyhow!("invalid CA key: {e}"))?;

        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.distinguished_name.push(DnType::CommonName, "pact-ca");
        let ca_cert = ca_params
            .self_signed(&ca_key_pair)
            .map_err(|e| anyhow::anyhow!("failed to reconstruct CA cert: {e}"))?;

        info!("loaded CA from disk for enrollment signing");

        Ok(Self {
            ca_certified_key: CertifiedKey { cert: ca_cert, key_pair: ca_key_pair },
            cert_lifetime_seconds,
        })
    }

    /// Get the CA certificate PEM (for distributing to agents in enrollment response).
    #[must_use]
    pub fn ca_cert_pem(&self) -> String {
        self.ca_certified_key.cert.pem()
    }

    /// Create a self-signed test CA for unit tests.
    pub fn test_ca() -> Self {
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.distinguished_name.push(DnType::CommonName, "pact-test-ca");
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        Self {
            ca_certified_key: CertifiedKey { cert: ca_cert, key_pair: ca_key },
            cert_lifetime_seconds: 259_200,
        }
    }

    /// Sign a node enrollment CSR (F11 fix).
    ///
    /// Parses the agent's CSR to extract its public key, then signs a
    /// certificate embedding that key. The agent's private key never
    /// leaves the agent — only the public key arrives via the CSR.
    ///
    /// If `csr_der` is empty (legacy/test path), falls back to generating
    /// a server-side keypair for backward compatibility.
    pub fn sign_csr(
        &self,
        csr_der: &[u8],
        node_id: &str,
        domain_id: &str,
    ) -> anyhow::Result<SignedCertResult> {
        let serial_uuid = uuid::Uuid::new_v4();
        let serial_hex = serial_uuid.to_string();

        let cert_pem = if csr_der.is_empty() {
            // Legacy/test path: no CSR provided, generate server-side keypair.
            // This is the fallback for tests and migrations.
            let mut params = CertificateParams::default();
            params
                .distinguished_name
                .push(DnType::CommonName, format!("pact-service-agent/{node_id}@{domain_id}"));
            let node_key = KeyPair::generate()
                .map_err(|e| anyhow::anyhow!("keypair generation failed: {e}"))?;
            let signed = params
                .signed_by(
                    &node_key,
                    &self.ca_certified_key.cert,
                    &self.ca_certified_key.key_pair,
                )
                .map_err(|e| anyhow::anyhow!("certificate signing failed: {e}"))?;
            debug!(node_id, serial = %serial_hex, "Signed certificate (legacy: server-generated key)");
            signed.pem()
        } else {
            // Real path: parse CSR, extract agent's public key, sign with CA.
            let csr_params =
                rcgen::CertificateSigningRequestParams::from_der(&csr_der.into())
                    .map_err(|e| anyhow::anyhow!("CSR parsing failed: {e}"))?;

            // Override the DN to include pact identity regardless of what the CSR says
            let mut params = csr_params.params;
            params
                .distinguished_name
                .push(DnType::CommonName, format!("pact-service-agent/{node_id}@{domain_id}"));

            // Sign with the agent's public key (from CSR) and our CA
            let signed = rcgen::CertificateSigningRequestParams {
                params,
                public_key: csr_params.public_key,
            }
            .signed_by(
                &self.ca_certified_key.cert,
                &self.ca_certified_key.key_pair,
            )
            .map_err(|e| anyhow::anyhow!("certificate signing failed: {e}"))?;

            debug!(node_id, serial = %serial_hex, "Signed certificate from agent CSR");
            signed.pem()
        };

        let ca_pem = self.ca_certified_key.cert.pem();
        let expires_at =
            chrono::Utc::now() + chrono::Duration::seconds(i64::from(self.cert_lifetime_seconds));

        Ok(SignedCertResult {
            cert_pem,
            ca_chain_pem: ca_pem,
            cert_serial: serial_hex,
            cert_expires_at: expires_at.to_rfc3339(),
        })
    }
}

/// Result of signing an enrollment certificate.
#[derive(Debug)]
pub struct SignedCertResult {
    pub cert_pem: String,
    pub ca_chain_pem: String,
    pub cert_serial: String,
    pub cert_expires_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ca_signs_cert_legacy() {
        let ca = CaKeyManager::test_ca();

        // Legacy path: empty CSR → server-side keypair
        let result = ca.sign_csr(&[], "node-001", "site-alpha").unwrap();
        assert!(!result.cert_pem.is_empty());
        assert!(result.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(!result.ca_chain_pem.is_empty());
        assert!(!result.cert_serial.is_empty());
        assert!(!result.cert_expires_at.is_empty());
    }

    #[test]
    fn test_ca_signs_real_csr() {
        let ca = CaKeyManager::test_ca();

        // Generate a real CSR from an agent-side keypair
        let agent_key = KeyPair::generate().unwrap();
        let mut csr_params = CertificateParams::default();
        csr_params
            .distinguished_name
            .push(DnType::CommonName, "agent-test-node");
        let csr = csr_params.serialize_request(&agent_key).unwrap();
        let csr_der = csr.der().to_vec();

        // Sign the CSR with the CA
        let result = ca.sign_csr(&csr_der, "node-001", "site-alpha").unwrap();
        assert!(!result.cert_pem.is_empty());
        assert!(result.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(!result.ca_chain_pem.is_empty());
    }

    #[test]
    fn test_ca_rejects_invalid_csr() {
        let ca = CaKeyManager::test_ca();
        let result = ca.sign_csr(b"not-a-valid-csr", "node-001", "site-alpha");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("CSR parsing failed"));
    }

    #[test]
    fn test_ca_cert_serial_is_unique() {
        let ca = CaKeyManager::test_ca();
        let r1 = ca.sign_csr(&[], "node-001", "domain-1").unwrap();
        let r2 = ca.sign_csr(&[], "node-002", "domain-1").unwrap();
        assert_ne!(r1.cert_serial, r2.cert_serial);
    }
}
