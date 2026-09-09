//! Shared test helpers and test module declarations for full_sync tests.

use super::{DeletedTargetLww, LiveTargetLww};

///   测试用空 catalog snapshot。
pub(crate) fn test_empty_catalog_snapshot() -> crate::sync::types::RemoteTargetCatalogSnapshot {
    crate::sync::types::RemoteTargetCatalogSnapshot {
        catalog: crate::sync::types::TargetLifecycleCatalog::default(),
        version: crate::sync::provider::model::RemoteVersion::new("__nonexistent__"),
    }
}

/// 模拟 catalog 路径写入始终失败的 Provider（用于测试 catalog 写失败时的 Retry 语义）。
pub(crate) struct AlwaysFailCatalogProvider {
    inner: crate::sync::provider::memory::MemoryProvider,
}
impl AlwaysFailCatalogProvider {
    pub(crate) fn new() -> Self {
        Self {
            inner: crate::sync::provider::memory::MemoryProvider::with_entries([(
                "projects/p1/chapter.md".to_string(),
                b"hello".to_vec(),
            )]),
        }
    }
}
impl crate::sync::provider::SyncProvider for AlwaysFailCatalogProvider {
    fn capabilities(&self) -> crate::sync::provider::capabilities::SyncCapabilities {
        self.inner.capabilities()
    }
    fn list(
        &self,
        prefix: &str,
    ) -> Result<
        Vec<crate::sync::provider::model::RemoteEntry>,
        crate::sync::provider::error::ProviderError,
    > {
        self.inner.list(prefix)
    }
    fn read(
        &self,
        path: &str,
    ) -> Result<
        Option<crate::sync::provider::model::RemoteObject>,
        crate::sync::provider::error::ProviderError,
    > {
        self.inner.read(path)
    }
    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: crate::sync::provider::model::WritePrecondition,
    ) -> Result<
        crate::sync::provider::model::RemoteVersion,
        crate::sync::provider::error::ProviderError,
    > {
        if path == crate::sync::target_lifecycle::TARGET_CATALOG_REMOTE_PATH {
            return Err(crate::sync::provider::error::ProviderError::Other {
                reason: "catalog write always fails".to_string(),
            });
        }
        self.inner.write(path, content, precondition)
    }
    fn delete(
        &self,
        path: &str,
        precondition: crate::sync::provider::model::DeletePrecondition,
    ) -> Result<(), crate::sync::provider::error::ProviderError> {
        self.inner.delete(path, precondition)
    }
}

/// 构造一个 deleted target LWW 元数据。
pub(crate) fn lww(deleted_at_ms: i64, device_id: &str) -> DeletedTargetLww {
    DeletedTargetLww {
        deleted_at_ms,
        device_id: device_id.to_string(),
    }
}

mod aggregate;
mod generation;
mod plan;
mod transfer;
