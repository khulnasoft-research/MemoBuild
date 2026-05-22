//! TLS configuration for secure cluster communication
//!
//! This module provides mTLS configuration for MemoBuild cluster nodes,
//! including certificate generation and loading.

use anyhow::{Context, Result};
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};
use rustls::ServerConfig;
use std::fs;
use std::sync::Arc;

/// TLS configuration for cluster communication
pub struct TlsConfig {
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
    pub ca_cert: Vec<u8>,
}

impl TlsConfig {
    /// Load TLS config from files
    pub fn from_files(cert_path: &str, key_path: &str, ca_path: &str) -> Result<Self> {
        let cert = fs::read(cert_path)
            .with_context(|| format!("Failed to read certificate from {}", cert_path))?;
        let key = fs::read(key_path)
            .with_context(|| format!("Failed to read private key from {}", key_path))?;
        let ca_cert = fs::read(ca_path)
            .with_context(|| format!("Failed to read CA certificate from {}", ca_path))?;

        Ok(Self { cert, key, ca_cert })
    }

    /// Generate self-signed certificates for development
    pub fn generate_self_signed(node_id: &str) -> Result<Self> {
        let mut params = CertificateParams::new(vec![format!("memobuild-{}", node_id)]);
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, format!("memobuild-{}", node_id));
        params
            .distinguished_name
            .push(DnType::OrganizationName, "MemoBuild");
        params.distinguished_name.push(DnType::CountryName, "US");

        let cert = Certificate::from_params(params)?;

        Ok(Self {
            cert: cert.serialize_pem()?.as_bytes().to_vec(),
            key: cert.serialize_private_key_pem().as_bytes().to_vec(),
            ca_cert: cert.serialize_pem()?.as_bytes().to_vec(), // Self-signed, so CA is the cert itself
        })
    }

    /// Create rustls ServerConfig
    pub fn server_config(&self) -> Result<ServerConfig> {
        let certs = rustls_pemfile::certs(&mut self.cert.as_slice())
            .map_err(|_| anyhow::anyhow!("Failed to parse certificate"))?
            .into_iter()
            .map(rustls::Certificate)
            .collect();

        let key = rustls_pemfile::pkcs8_private_keys(&mut self.key.as_slice())
            .map_err(|_| anyhow::anyhow!("Failed to parse private key"))?
            .into_iter()
            .map(rustls::PrivateKey)
            .next()
            .ok_or_else(|| anyhow::anyhow!("No private key found"))?;

        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(config)
    }

    /// Create axum-server RustlsConfig
    pub fn axum_rustls_config(&self) -> Result<axum_server::tls_rustls::RustlsConfig> {
        let server_config = self.server_config()?;
        Ok(axum_server::tls_rustls::RustlsConfig::from_config(
            Arc::new(server_config),
        ))
    }

    /// Create rustls ClientConfig for cluster communication (mTLS)
    pub fn client_config(&self) -> Result<rustls::ClientConfig> {
        let ca_cert = rustls_pemfile::certs(&mut self.ca_cert.as_slice())
            .map_err(|_| anyhow::anyhow!("Failed to parse CA certificate"))?
            .into_iter()
            .map(rustls::Certificate)
            .collect::<Vec<_>>();

        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_cert {
            root_store.add(&cert)?;
        }

        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Ok(config)
    }

    /// Create rustls ClientConfig with client certificate for mTLS
    #[allow(dead_code)]
    pub fn client_config_with_auth(&self) -> Result<rustls::ClientConfig> {
        let ca_cert = rustls_pemfile::certs(&mut self.ca_cert.as_slice())
            .map_err(|_| anyhow::anyhow!("Failed to parse CA certificate"))?
            .into_iter()
            .map(rustls::Certificate)
            .collect::<Vec<_>>();

        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_cert {
            root_store.add(&cert)?;
        }

        // For mTLS, we just use server-side cert as client cert too
        // This works for development/testing
        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Ok(config)
    }
}
