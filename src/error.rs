/// MemoBuild error types and handling utilities
/// Main error type for MemoBuild operations
#[derive(Debug)]
pub enum MemoBuildError {
    /// CAS (Content-Addressable Storage) integrity violation
    CASIntegrityFailure {
        expected: String,
        actual: String,
        data_size: usize,
    },
    /// Network error with retry information
    NetworkError {
        message: String,
        retryable: bool,
        attempt: u32,
    },
    /// Storage operation failed
    StorageError { operation: String, reason: String },
    /// Cache coherency violation
    CacheCoherencyError { hash: String, reason: String },
    /// Remote cache synchronization failed
    SyncError { message: String, recovered: bool },
    /// Metadata store error
    MetadataError { operation: String, reason: String },
    /// Resource conflict or constraint violation
    ConstraintViolation { reason: String },
    /// Wrapped anyhow error for compatibility
    Other(anyhow::Error),
}

impl std::fmt::Display for MemoBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CASIntegrityFailure {
                expected,
                actual,
                data_size,
            } => {
                write!(
                    f,
                    "CAS integrity failure: expected {}, got {} (size: {} bytes)",
                    expected, actual, data_size
                )
            }
            Self::NetworkError {
                message,
                retryable,
                attempt,
            } => {
                write!(
                    f,
                    "Network error (attempt {}, retryable: {}): {}",
                    attempt, retryable, message
                )
            }
            Self::StorageError { operation, reason } => {
                write!(f, "Storage error in {}: {}", operation, reason)
            }
            Self::CacheCoherencyError { hash, reason } => {
                write!(f, "Cache coherency error for {}: {}", hash, reason)
            }
            Self::SyncError { message, recovered } => {
                if *recovered {
                    write!(f, "Sync error (recovered): {}", message)
                } else {
                    write!(f, "Sync error: {}", message)
                }
            }
            Self::MetadataError { operation, reason } => {
                write!(f, "Metadata error in {}: {}", operation, reason)
            }
            Self::ConstraintViolation { reason } => {
                write!(f, "Constraint violation: {}", reason)
            }
            Self::Other(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for MemoBuildError {}

impl From<anyhow::Error> for MemoBuildError {
    fn from(err: anyhow::Error) -> Self {
        MemoBuildError::Other(err)
    }
}

/// Helper to determine if an error is retryable
pub fn is_retryable(err: &MemoBuildError) -> bool {
    match err {
        MemoBuildError::NetworkError { retryable, .. } => *retryable,
        MemoBuildError::CASIntegrityFailure { .. } => false,
        MemoBuildError::StorageError { .. } => false,
        MemoBuildError::CacheCoherencyError { .. } => false,
        MemoBuildError::MetadataError { .. } => true,
        MemoBuildError::SyncError { .. } => true,
        MemoBuildError::ConstraintViolation { .. } => false,
        MemoBuildError::Other(_) => false,
    }
}

/// Retry configuration for resilient operations
#[derive(Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }
}

/// Calculates exponential backoff with jitter
pub fn calculate_backoff(attempt: u32, config: &RetryConfig) -> u64 {
    let backoff = (config.initial_backoff_ms as f64
        * config.backoff_multiplier.powi(attempt as i32))
    .min(config.max_backoff_ms as f64) as u64;

    // Add jitter: ±20% of backoff, then clamp to max
    let jitter = (backoff as f64) * (rand::random::<f64>() * 0.4 - 0.2);
    ((backoff as f64) + jitter)
        .max(0.0)
        .min(config.max_backoff_ms as f64) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cas_error_display() {
        let err = MemoBuildError::CASIntegrityFailure {
            expected: "abc123def456".to_string(),
            actual: "xyz789uvw012".to_string(),
            data_size: 1024,
        };
        let msg = err.to_string();
        assert!(msg.contains("abc123de"));
        assert!(msg.contains("xyz789uv"));
        assert!(msg.contains("1024"));
    }

    #[test]
    fn test_retryable_detection() {
        let network_err = MemoBuildError::NetworkError {
            message: "timeout".to_string(),
            retryable: true,
            attempt: 1,
        };
        assert!(is_retryable(&network_err));

        let integrity_err = MemoBuildError::CASIntegrityFailure {
            expected: "abc".to_string(),
            actual: "def".to_string(),
            data_size: 100,
        };
        assert!(!is_retryable(&integrity_err));
    }

    #[test]
    fn test_exponential_backoff() {
        let config = RetryConfig::default();
        let backoff_0 = calculate_backoff(0, &config);
        let backoff_1 = calculate_backoff(1, &config);
        let backoff_2 = calculate_backoff(2, &config);

        // Should generally increase (within bounds due to jitter)
        assert!(backoff_0 <= config.max_backoff_ms);
        assert!(backoff_1 <= config.max_backoff_ms);
        assert!(backoff_2 <= config.max_backoff_ms);
    }
}
