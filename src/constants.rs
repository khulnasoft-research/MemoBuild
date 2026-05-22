// Centralized constants for the MemoBuild project to avoid magic numbers

/// Default timeout for remote execution nodes in seconds (10 minutes)
pub const DEFAULT_REMOTE_EXECUTION_TIMEOUT_SECS: u64 = 600;

/// Default buffer size for IO operations (8 MB)
pub const DEFAULT_IO_BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// Default maximum number of retry attempts for remote cache
pub const DEFAULT_MAX_RETRY_ATTEMPTS: u32 = 3;

/// Default initial backoff for requests in milliseconds
pub const DEFAULT_INITIAL_BACKOFF_MS: u64 = 100;

/// Default maximum backoff limit in milliseconds
pub const DEFAULT_MAX_BACKOFF_MS: u64 = 5000;

/// Standard size used for batching artifact hashes
pub const ARTIFACT_BATCH_SIZE: usize = 50;

/// Number of days before artifacts are garbage collected
pub const DEFAULT_GC_DAYS: u32 = 7;

/// WebSocket broadcast channel capacity
pub const MAX_WS_BROADCAST_CAPACITY: usize = 100;

/// Number of past builds to return for analytics queries
pub const ANALYTICS_DB_LIMIT: usize = 50;
