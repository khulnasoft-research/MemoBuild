/// Unit tests for error handling improvements
#[cfg(test)]
mod tests {
    use memobuild::error::{calculate_backoff, is_retryable, MemoBuildError, RetryConfig};

    #[test]
    fn test_cas_integrity_check() {
        // Test that CAS integrity errors are properly detected
        let expected_hash = "abc123def456abc123def456abc123de".to_string();
        let actual_hash = "xyz789uvw012xyz789uvw012xyz789uv".to_string();
        let data_size = 2048;

        let err = MemoBuildError::CASIntegrityFailure {
            expected: expected_hash.clone(),
            actual: actual_hash.clone(),
            data_size,
        };

        let msg = err.to_string();
        assert!(msg.contains("integrity failure"));
        assert!(msg.contains("2048"));
        assert!(!is_retryable(&err));
    }

    #[test]
    fn test_network_error_retryable() {
        let network_err = MemoBuildError::NetworkError {
            message: "connection timeout".to_string(),
            retryable: true,
            attempt: 1,
        };

        assert!(is_retryable(&network_err));

        let non_retryable = MemoBuildError::NetworkError {
            message: "connection refused".to_string(),
            retryable: false,
            attempt: 1,
        };

        assert!(!is_retryable(&non_retryable));
    }

    #[test]
    fn test_backoff_calculation() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            backoff_multiplier: 2.0,
        };

        for attempt in 0..5 {
            let backoff = calculate_backoff(attempt, &config);
            assert!(backoff <= config.max_backoff_ms);
            assert!(backoff > 0);
        }
    }

    #[test]
    fn test_backoff_respects_max() {
        let config = RetryConfig {
            max_attempts: 10,
            initial_backoff_ms: 100,
            max_backoff_ms: 1000,
            backoff_multiplier: 10.0,
        };

        for attempt in 0..10 {
            let backoff = calculate_backoff(attempt, &config);
            assert!(
                backoff <= config.max_backoff_ms,
                "Backoff {} exceeded max {}",
                backoff,
                config.max_backoff_ms
            );
        }
    }

    #[test]
    fn test_error_conversion() {
        let cache_err = MemoBuildError::CASIntegrityFailure {
            expected: "exp".to_string(),
            actual: "act".to_string(),
            data_size: 100,
        };

        let anyhow_err: anyhow::Error = cache_err.into();
        assert!(anyhow_err.to_string().contains("integrity"));
    }

    #[test]
    fn test_storage_error_display() {
        let err = MemoBuildError::StorageError {
            operation: "write".to_string(),
            reason: "disk full".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("write"));
        assert!(msg.contains("disk full"));
    }

    #[test]
    fn test_cache_coherency_error_display() {
        let err = MemoBuildError::CacheCoherencyError {
            hash: "abc123".to_string(),
            reason: "conflicting updates".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("abc123"));
        assert!(msg.contains("conflicting"));
    }

    #[test]
    fn test_sync_error_recovered_state() {
        let recovered_err = MemoBuildError::SyncError {
            message: "partial sync".to_string(),
            recovered: true,
        };
        let msg = recovered_err.to_string();
        assert!(msg.contains("recovered"));

        let unrecovered_err = MemoBuildError::SyncError {
            message: "complete failure".to_string(),
            recovered: false,
        };
        let msg = unrecovered_err.to_string();
        assert!(!msg.contains("recovered"));
    }
}
