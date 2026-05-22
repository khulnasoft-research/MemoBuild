//! Audit Trail
//!
//! This module provides an immutable append-only audit log for MemoBuild operations.
//! It implements a tamper-evident log with SHA256 chain hashing.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    last_hash: String,
}

impl AuditLog {
    pub fn new() -> Self {
        Self {
            entries: vec![],
            last_hash: "0".repeat(64), // Genesis hash
        }
    }

    pub fn append(&mut self, event: AuditEvent) -> Result<()> {
        let entry = AuditEntry::new(event, &self.last_hash)?;
        self.last_hash = entry.hash.clone();
        self.entries.push(entry);
        Ok(())
    }

    pub fn verify(&self) -> Result<bool> {
        let mut prev_hash = "0".repeat(64);
        
        for entry in &self.entries {
            // Verify the entry's previous hash
            if entry.prev_hash != prev_hash {
                return Ok(false);
            }
            
            // Verify the entry's hash
            let computed = entry.compute_hash()?;
            if computed != entry.hash {
                return Ok(false);
            }
            
            prev_hash = entry.hash.clone();
        }
        
        Ok(true)
    }

    pub fn query(&self, filter: &AuditFilter) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| filter.matches(e))
            .collect()
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: EventType,
    pub actor: String,
    pub resource: String,
    pub action: String,
    pub details: serde_json::Value,
    pub prev_hash: String,
    pub hash: String,
}

impl AuditEntry {
    pub fn new(event: AuditEvent, prev_hash: &str) -> Result<Self> {
        let mut entry = Self {
            timestamp: Utc::now(),
            event_type: event.event_type,
            actor: event.actor,
            resource: event.resource,
            action: event.action,
            details: event.details,
            prev_hash: prev_hash.to_string(),
            hash: String::new(),
        };
        
        entry.hash = entry.compute_hash()?;
        Ok(entry)
    }

    pub fn compute_hash(&self) -> Result<String> {
        let mut hasher = Sha256::new();
        
        hasher.update(self.timestamp.to_rfc3339().as_bytes());
        hasher.update(self.event_type.to_string().as_bytes());
        hasher.update(self.actor.as_bytes());
        hasher.update(self.resource.as_bytes());
        hasher.update(self.action.as_bytes());
        hasher.update(serde_json::to_string(&self.details)?.as_bytes());
        hasher.update(self.prev_hash.as_bytes());
        
        Ok(hex::encode(hasher.finalize()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_type: EventType,
    pub actor: String,
    pub resource: String,
    pub action: String,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    CacheRead,
    CacheWrite,
    NodeJoin,
    NodeLeave,
    GcRun,
    ScaleUp,
    ScaleDown,
    TokenCreated,
    TokenRevoked,
    BuildStarted,
    BuildCompleted,
    BuildFailed,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::CacheRead => write!(f, "cache.read"),
            EventType::CacheWrite => write!(f, "cache.write"),
            EventType::NodeJoin => write!(f, "cluster.node.join"),
            EventType::NodeLeave => write!(f, "cluster.node.leave"),
            EventType::GcRun => write!(f, "gc.run"),
            EventType::ScaleUp => write!(f, "scaling.up"),
            EventType::ScaleDown => write!(f, "scaling.down"),
            EventType::TokenCreated => write!(f, "auth.token.created"),
            EventType::TokenRevoked => write!(f, "auth.token.revoked"),
            EventType::BuildStarted => write!(f, "build.started"),
            EventType::BuildCompleted => write!(f, "build.completed"),
            EventType::BuildFailed => write!(f, "build.failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditFilter {
    pub event_type: Option<EventType>,
    pub actor: Option<String>,
    pub resource: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

impl AuditFilter {
    pub fn matches(&self, entry: &AuditEntry) -> bool {
        if let Some(ref et) = self.event_type {
            if &entry.event_type != et {
                return false;
            }
        }
        
        if let Some(ref a) = self.actor {
            if &entry.actor != a {
                return false;
            }
        }
        
        if let Some(ref r) = self.resource {
            if !entry.resource.contains(r) {
                return false;
            }
        }
        
        if let Some(ref start) = self.start_time {
            if entry.timestamp < *start {
                return false;
            }
        }
        
        if let Some(ref end) = self.end_time {
            if entry.timestamp > *end {
                return false;
            }
        }
        
        true
    }
}

/// Audit logger that can be used in the application
pub struct AuditLogger {
    log: std::sync::Arc<parking_lot::RwLock<AuditLog>>,
}

impl AuditLogger {
    pub fn new() -> Self {
        Self {
            log: std::sync::Arc::new(parking_lot::RwLock::new(AuditLog::new())),
        }
    }

    pub fn log_event(&self, event: AuditEvent) {
        let mut log = self.log.write();
        if let Err(e) = log.append(event) {
            eprintln!("Failed to append audit entry: {}", e);
        }
    }

    pub fn query(&self, filter: &AuditFilter) -> Vec<AuditEntry> {
        let log = self.log.read();
        log.query(filter).into_iter().cloned().collect()
    }

    pub fn verify(&self) -> bool {
        let log = self.log.read();
        log.verify().unwrap_or(false)
    }

    pub fn export(&self, format: ExportFormat) -> Result<String> {
        let log = self.log.read();
        
        match format {
            ExportFormat::Json => {
                serde_json::to_string_pretty(&*log)
                    .map_err(|e| anyhow::anyhow!("Failed to export: {}", e))
            }
            ExportFormat::Csv => {
                let mut csv = String::new();
                csv.push_str("timestamp,event_type,actor,resource,action,details\n");
                
                for entry in &log.entries {
                    csv.push_str(&format!("{},{},{},{},{},{}\n",
                        entry.timestamp.to_rfc3339(),
                        entry.event_type,
                        entry.actor,
                        entry.resource,
                        entry.action,
                        serde_json::to_string(&entry.details).unwrap_or_default()
                    ));
                }
                
                Ok(csv)
            }
            ExportFormat::Ndjson => {
                let mut ndjson = String::new();
                
                for entry in &log.entries {
                    ndjson.push_str(&serde_json::to_string(entry)?);
                    ndjson.push('\n');
                }
                
                Ok(ndjson)
            }
        }
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum ExportFormat {
    Json,
   Csv,
    Ndjson,
}

/// Helper function to log cache operations
pub fn log_cache_access(logger: &AuditLogger, hash: &str, is_write: bool) {
    logger.log_event(AuditEvent {
        event_type: if is_write { EventType::CacheWrite } else { EventType::CacheRead },
        actor: "system".to_string(),
        resource: format!("/cache/{}", hash),
        action: if is_write { "PUT".to_string() } else { "GET".to_string() },
        details: serde_json::json!({ "hash": hash }),
    });
}

/// Helper function to log build events
pub fn log_build_event(logger: &AuditLogger, build_id: &str, status: &str) {
    let event_type = match status {
        "started" => EventType::BuildStarted,
        "completed" => EventType::BuildCompleted,
        _ => EventType::BuildFailed,
    };
    
    logger.log_event(AuditEvent {
        event_type,
        actor: "system".to_string(),
        resource: format!("/build/{}", build_id),
        action: status.to_string(),
        details: serde_json::json!({ "build_id": build_id }),
    });
}