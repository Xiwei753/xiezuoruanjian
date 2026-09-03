//! 内存 Provider — 用于测试和本地无远端的场景。
//!
//! [`MemoryProvider`] 用进程内 `Mutex<HashMap>` 存储远端对象，
//! 实现 [`SyncProvider`] trait 的全部方法。
//!
//! ## 语义
//!
//! - `list(prefix)`：返回路径以 `prefix + "/"` 开头的条目，剥掉前缀后返回。
//!   `prefix` 为空时返回全部条目（不剥前缀）。
//! - `read(path)`：返回完整 path 对应的对象内容。
//! - `write(path, content, precondition)`：根据 precondition 检查后写入，
//!   生成新的 `RemoteVersion`（UUID）。
//! - `delete(path, precondition)`：根据 precondition 检查后删除。
//!
//! ## 线程安全
//!
//! 内部用 `Mutex` 保护，`SyncProvider` 要求 `Send + Sync`，
//! `Mutex` 天然满足。锁粒度为整个 HashMap，不追求并发性能（仅用于测试/本地）。
//! 锁中毒（持有锁的线程 panic）显式映射为 `ProviderError::Other`，不使用 `expect`。

use std::collections::HashMap;
use std::sync::Mutex;

use super::capabilities::SyncCapabilities;
use super::error::ProviderError;
use super::model::{
    DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion, WritePrecondition,
};
use super::SyncProvider;

/// 内存 Provider 存储：path → (content, version)。
type Store = HashMap<String, (Vec<u8>, RemoteVersion)>;

/// 锁中毒错误文案。
const LOCK_POISONED: &str = "memory provider lock poisoned";

/// 内存 Provider — 进程内 HashMap 存储，用于测试和本地无远端场景。
///
/// 创建时为空，通过 `write`/`delete` 修改内容。
/// 所有操作在锁内同步完成，无网络延迟。
#[derive(Debug, Default)]
pub struct MemoryProvider {
    store: Mutex<Store>,
}

impl MemoryProvider {
    /// 创建空的内存 Provider。
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }

    /// 从初始条目创建内存 Provider（用于测试夹具）。
    ///
    /// `entries` 为 (path, content) 列表，每条生成一个 UUID 版本。
    pub fn with_entries(entries: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        let provider = Self::new();
        {
            // 刚构造的 Mutex 不会中毒；若中毒也显式恢复 guard（构造阶段无并发风险）。
            let mut store = provider.store.lock().unwrap_or_else(|e| e.into_inner());
            for (path, content) in entries {
                let version = RemoteVersion(uuid::Uuid::new_v4().to_string());
                store.insert(path, (content, version));
            }
        }
        provider
    }

    /// 生成新版本标识（UUID v4）。
    fn new_version() -> RemoteVersion {
        RemoteVersion(uuid::Uuid::new_v4().to_string())
    }

    /// 把锁中毒转为 `ProviderError::Other`。
    fn lock_err() -> ProviderError {
        ProviderError::Other {
            reason: LOCK_POISONED.to_string(),
        }
    }
}

impl SyncProvider for MemoryProvider {
    fn capabilities(&self) -> SyncCapabilities {
        SyncCapabilities::memory()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let store = self.store.lock().map_err(|_| Self::lock_err())?;
        let needle = if prefix.is_empty() {
            None
        } else {
            Some(format!("{prefix}/"))
        };
        let mut entries = Vec::new();
        for (path, (_, version)) in store.iter() {
            let entry_path = match &needle {
                None => Some(path.clone()),
                Some(n) => path.strip_prefix(n).map(|s| s.to_string()),
            };
            if let Some(p) = entry_path {
                entries.push(RemoteEntry {
                    path: p,
                    version: version.clone(),
                });
            }
        }
        // 路径排序，保证测试可重现。
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        let store = self.store.lock().map_err(|_| Self::lock_err())?;
        match store.get(path) {
            Some((content, version)) => Ok(Some(RemoteObject {
                path: path.to_string(),
                content: content.clone(),
                version: version.clone(),
            })),
            None => Ok(None),
        }
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        let mut store = self.store.lock().map_err(|_| Self::lock_err())?;
        match precondition {
            WritePrecondition::IfMatch(expected) => match store.get(path) {
                Some((_, current)) if *current == expected => {}
                Some((_, current)) => {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!("version mismatch: expected={expected}, current={current}"),
                    });
                }
                None => {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: "object does not exist".to_string(),
                    });
                }
            },
            WritePrecondition::CreateNew => {
                if store.contains_key(path) {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: "object already exists".to_string(),
                    });
                }
            }
            WritePrecondition::Unconditional => {}
        }
        let version = Self::new_version();
        store.insert(path.to_string(), (content.to_vec(), version.clone()));
        Ok(version)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        let mut store = self.store.lock().map_err(|_| Self::lock_err())?;
        match precondition {
            DeletePrecondition::IfMatch(expected) => match store.get(path) {
                Some((_, current)) if *current == expected => {}
                Some((_, current)) => {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!("version mismatch: expected={expected}, current={current}"),
                    });
                }
                None => {
                    return Err(ProviderError::NotFound {
                        path: path.to_string(),
                    });
                }
            },
            DeletePrecondition::Unconditional => {}
        }
        store.remove(path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_roundtrip() {
        let p = MemoryProvider::new();
        let v = p
            .write("a/b.md", b"hello", WritePrecondition::CreateNew)
            .unwrap();
        assert!(!v.as_str().is_empty());
        let obj = p.read("a/b.md").unwrap().unwrap();
        assert_eq!(obj.content, b"hello");
        assert_eq!(obj.version, v);
    }

    #[test]
    fn create_new_rejects_existing() {
        let p = MemoryProvider::new();
        p.write("x", b"1", WritePrecondition::CreateNew).unwrap();
        let err = p
            .write("x", b"2", WritePrecondition::CreateNew)
            .unwrap_err();
        assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
    }

    #[test]
    fn if_match_rejects_stale() {
        let p = MemoryProvider::new();
        let v1 = p.write("x", b"1", WritePrecondition::CreateNew).unwrap();
        let _v2 = p
            .write("x", b"2", WritePrecondition::IfMatch(v1.clone()))
            .unwrap();
        // 用旧版本写入应失败
        let err = p
            .write("x", b"3", WritePrecondition::IfMatch(v1))
            .unwrap_err();
        assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
    }

    #[test]
    fn list_strips_prefix() {
        let p = MemoryProvider::with_entries([
            ("projects/p1/a.md".to_string(), b"a".to_vec()),
            ("projects/p1/b.md".to_string(), b"b".to_vec()),
            ("projects/p2/c.md".to_string(), b"c".to_vec()),
        ]);
        let entries = p.list("projects/p1").unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["a.md", "b.md"]);
    }

    #[test]
    fn list_empty_prefix_returns_all() {
        let p = MemoryProvider::with_entries([
            ("a".to_string(), b"".to_vec()),
            ("b".to_string(), b"".to_vec()),
        ]);
        let entries = p.list("").unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn delete_with_precondition() {
        let p = MemoryProvider::new();
        let v = p.write("x", b"1", WritePrecondition::CreateNew).unwrap();
        p.delete("x", DeletePrecondition::IfMatch(v)).unwrap();
        assert!(p.read("x").unwrap().is_none());
    }

    #[test]
    fn capabilities_are_memory() {
        let p = MemoryProvider::new();
        let cap = p.capabilities();
        assert!(cap.atomic_write);
        assert!(cap.batch);
        assert!(!cap.server_timestamp);
    }
}
