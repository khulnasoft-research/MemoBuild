//! Secrets Management Integration
//!
//! This module provides a trait for secret providers, allowing MemoBuild
//! to integrate with various secret management systems like HashiCorp Vault,
//! AWS KMS, or environment variables.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

pub mod aws;

/// Trait for secret providers
#[async_trait]
pub trait SecretProvider: Send + Sync {
    async fn get(&self, _key: &str) -> Result<String>;
    async fn set(&self, _key: &str, _value: &str) -> Result<()>;
    async fn list(&self) -> Result<Vec<String>>;
}

/// Environment variable secret provider (default, dev only)
pub struct EnvSecretProvider {
    prefix: String,
}

impl EnvSecretProvider {
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }
}

#[async_trait]
impl SecretProvider for EnvSecretProvider {
    async fn get(&self, key: &str) -> Result<String> {
        let env_key = format!("{}_{}", self.prefix, key.to_uppercase());
        std::env::var(&env_key).map_err(|_| anyhow::anyhow!("Secret {} not found", key))
    }

    async fn set(&self, key: &str, value: &str) -> Result<()> {
        std::env::set_var(format!("{}_{}", self.prefix, key.to_uppercase()), value);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
}

/// HashiCorp Vault secret provider
pub struct VaultSecretProvider {
    client: Arc<tokio::sync::Mutex<Option<vaultrs::client::VaultClient>>>,
    mount: String,
    path: String,
    token: String,
    address: String,
}

impl VaultSecretProvider {
    pub fn new(vault_addr: &str, token: &str, mount: String, path: String) -> Self {
        Self {
            client: Arc::new(tokio::sync::Mutex::new(None)),
            mount,
            path,
            token: token.to_string(),
            address: vault_addr.to_string(),
        }
    }

    async fn get_client(&self) -> Result<tokio::sync::MutexGuard<'_, Option<vaultrs::client::VaultClient>>> {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            let client = vaultrs::client::VaultClient::new(
                vaultrs::client::VaultClientSettingsBuilder::default()
                    .address(&self.address)
                    .token(&self.token)
                    .build()?,
            )?;
            *guard = Some(client);
        }
        Ok(guard)
    }
}

#[async_trait]
impl SecretProvider for VaultSecretProvider {
    async fn get(&self, key: &str) -> Result<String> {
        let mut guard = self.get_client().await?;
        let client = guard.as_mut().ok_or_else(|| anyhow::anyhow!("Vault client not initialized"))?;
        let secret: HashMap<String, String> = vaultrs::kv2::read(client, &self.mount, &self.path).await?;
        secret.get(key).cloned().ok_or_else(|| anyhow::anyhow!("Secret {} not found", key))
    }

    async fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut guard = self.get_client().await?;
        let client = guard.as_mut().ok_or_else(|| anyhow::anyhow!("Vault client not initialized"))?;
        
        let mut secret = vaultrs::kv2::read::<HashMap<String, String>>(client, &self.mount, &self.path)
            .await
            .unwrap_or_default();
        secret.insert(key.to_string(), value.to_string());
        vaultrs::kv2::set(client, &self.mount, &self.path, &secret).await?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<String>> {
        let mut guard = self.get_client().await?;
        let client = guard.as_mut().ok_or_else(|| anyhow::anyhow!("Vault client not initialized"))?;
        let secret: HashMap<String, String> = vaultrs::kv2::read(client, &self.mount, &self.path).await?;
        Ok(secret.keys().cloned().collect())
    }
}

/// AWS KMS provider for encryption at rest
pub struct AwsKmsProvider {
    _region: String,
    _key_id: String,
}

impl AwsKmsProvider {
    pub fn new(region: &str, key_id: &str) -> Self {
        Self {
            _region: region.to_string(),
            _key_id: key_id.to_string(),
        }
    }
}

#[async_trait]
impl SecretProvider for AwsKmsProvider {
    async fn get(&self, _key: &str) -> Result<String> {
        Err(anyhow::anyhow!("AWS KMS get not implemented - use SecretsManager for key storage"))
    }

    async fn set(&self, _key: &str, _value: &str) -> Result<()> {
        Err(anyhow::anyhow!("AWS KMS set not implemented - use SecretsManager for key storage"))
    }

    async fn list(&self) -> Result<Vec<String>> {
        Err(anyhow::anyhow!("AWS KMS list not implemented"))
    }
}

/// Create a secret provider based on environment configuration
pub fn create_secret_provider() -> Result<Box<dyn SecretProvider>> {
    let provider_type = std::env::var("MEMOBUILD_SECRET_PROVIDER")
        .unwrap_or_else(|_| "env".to_string());

    match provider_type.as_str() {
        "vault" => {
            let addr = std::env::var("MEMOBUILD_VAULT_ADDR")?;
            let token = std::env::var("MEMOBUILD_VAULT_TOKEN")?;
            let mount = std::env::var("MEMOBUILD_VAULT_MOUNT").unwrap_or_else(|_| "secret".to_string());
            let path = std::env::var("MEMOBUILD_VAULT_PATH").unwrap_or_else(|_| "memobuild".to_string());
            Ok(Box::new(VaultSecretProvider::new(&addr, &token, mount, path)))
        }
        "aws" => {
            let region = std::env::var("MEMOBUILD_AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
            let key_id = std::env::var("MEMOBUILD_AWS_KMS_KEY_ID")?;
            Ok(Box::new(AwsKmsProvider::new(&region, &key_id)))
        }
        _ => Ok(Box::new(EnvSecretProvider::new("memobuild".to_string()))),
    }
}