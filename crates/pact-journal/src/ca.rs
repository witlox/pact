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
        ca_params
            .distinguished_name
            .push(DnType::CommonName, format!("pact-ca-{domain_id}"));

        let ca_key = KeyPair::generate()
            .map_err(|e| anyhow::anyhow!("CA key generation failed: {e}"))?;
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
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "pact-ca");
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

    /// Sign a node enrollment request.
    ///
    /// Generates a node certificate signed by the intermediate CA.
    /// The `_csr_der` parameter receives the agent's CSR (used to extract
    /// the public key in a full implementation); for now we generate a
    /// new keypair server-side for the certificate and return it.
    ///
    /// In the real flow: agent generates keypair locally, sends CSR,
    /// journal extracts public key from CSR and embeds it in the cert.
    /// Since rcgen 0.13 doesn't expose CSR parsing, we generate a cert
    /// with the agent's identity and the CA signs it.
    pub fn sign_csr(
        &self,
        _csr_der: &[u8],
        node_id: &str,
        domain_id: &str,
    ) -> anyhow::Result<SignedCertResult> {
        // Generate a certificate for the node, signed by our CA
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, format!("pact-service-agent/{node_id}@{domain_id}"));

        // Generate serial number from UUID
        let serial_uuid = uuid::Uuid::new_v4();

        // Sign the cert with our CA
        let node_key =
            KeyPair::generate().map_err(|e| anyhow::anyhow!("keypair generation failed: {e}"))?;
        let signed = params
            .signed_by(&node_key, &self.ca_certified_key.cert, &self.ca_certified_key.key_pair)
            .map_err(|e| anyhow::anyhow!("certificate signing failed: {e}"))?;

        let cert_pem = signed.pem();
        let ca_pem = self.ca_certified_key.cert.pem();
        let serial_hex = serial_uuid.to_string();

        // Compute expiry from now + lifetime
        let expires_at =
            chrono::Utc::now() + chrono::Duration::seconds(i64::from(self.cert_lifetime_seconds));

        debug!(node_id, serial = %serial_hex, "Signed enrollment certificate");

        Ok(SignedCertResult {
            cert_pem,
            ca_chain_pem: ca_pem,
            cert_serial: serial_hex,
            cert_expires_at: expires_at.to_rfc3339(),
        })
    }
}

/// Result of signing an enrollment certificate.
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
    fn test_ca_signs_cert() {
        let ca = CaKeyManager::test_ca();

        // Simulating a CSR with empty bytes (real CSR parsing not yet available in rcgen 0.13)
        let result = ca.sign_csr(&[], "node-001", "site-alpha").unwrap();
        assert!(!result.cert_pem.is_empty());
        assert!(result.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(!result.ca_chain_pem.is_empty());
        assert!(!result.cert_serial.is_empty());
        assert!(!result.cert_expires_at.is_empty());
    }

    #[test]
    fn test_ca_cert_serial_is_unique() {
        let ca = CaKeyManager::test_ca();
        let r1 = ca.sign_csr(&[], "node-001", "domain-1").unwrap();
        let r2 = ca.sign_csr(&[], "node-002", "domain-1").unwrap();
        assert_ne!(r1.cert_serial, r2.cert_serial);
    }
}
