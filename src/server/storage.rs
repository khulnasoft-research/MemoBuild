// Re-export from the top-level storage module.
// The `ArtifactStorage` trait and all backend implementations live in `crate::storage`.
pub use crate::storage::{ArtifactStorage, GcsStorage, LocalStorage, S3Storage, StorageBackend};
