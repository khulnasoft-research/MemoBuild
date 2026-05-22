//! in-toto DSSE signature support

use crate::slsa::Signature;
use anyhow::Result;
use serde_json::json;

/// Sign payload using DSSE (Dead Simple Signed Envelopes)
pub fn sign_dsse(payload: &str) -> Result<Signature> {
    // In production, this would use a proper signing key
    // For now, generate a placeholder signature
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Simple signature using SHA256 of payload + timestamp
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hasher.update(timestamp.to_le_bytes());
    let sig = hex::encode(hasher.finalize());

    Ok(Signature {
        keyid: Some(format!("memobuild-key-{}", timestamp)),
        sig,
    })
}

/// Verify DSSE signature
pub fn verify_dsse(_payload: &str, signature: &Signature) -> Result<bool> {
    // In production, this would verify against the public key
    // For now, just check signature format
    Ok(signature.sig.len() == 64)
}

/// Create an in-toto statement
pub fn create_statement(
    predicate_type: &str,
    predicate: serde_json::Value,
    subject: Vec<(String, String)>,
) -> serde_json::Value {
    json!({
        "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://slsa.dev/provenance/v1"
        ],
        "@type": "https://in-toto.io/Statement/v1",
        "predicateType": predicate_type,
        "predicate": predicate,
        "subject": subject.iter().map(|(name, digest)| {
            json!({
                "name": name,
                "digest": {
                    "sha256": digest
                }
            })
        }).collect::<Vec<_>>()
    })
}
