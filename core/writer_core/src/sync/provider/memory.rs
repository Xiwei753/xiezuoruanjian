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

    // ===== Unconditional 契约测试 =====
    //
    // Unconditional 语义：Provider 自己处理存在性，直接覆盖/删除。
    // 对 MemoryProvider 而言没有 SHA 约束，Unconditional 直接写入/删除即可。
    // 这组测试同时作为 GitHubProvider Unconditional 契约的语义参照：
    // GitHub 的 Unconditional 应表现为"存在则覆盖、不存在则创建；删除幂等"。

    #[test]
    fn unconditional_write_overwrites_existing() {
        let p = MemoryProvider::new();
        let v1 = p.write("x", b"old", WritePrecondition::CreateNew).unwrap();
        // Unconditional 覆盖已存在对象，不检查版本。
        let v2 = p
            .write("x", b"new", WritePrecondition::Unconditional)
            .unwrap();
        assert_ne!(v1, v2);
        let obj = p.read("x").unwrap().unwrap();
        assert_eq!(obj.content, b"new");
        assert_eq!(obj.version, v2);
    }

    #[test]
    fn unconditional_write_creates_when_absent() {
        let p = MemoryProvider::new();
        // Unconditional 写入不存在的对象 = 创建。
        let v = p
            .write("y", b"fresh", WritePrecondition::Unconditional)
            .unwrap();
        let obj = p.read("y").unwrap().unwrap();
        assert_eq!(obj.content, b"fresh");
        assert_eq!(obj.version, v);
    }

    #[test]
    fn unconditional_delete_existing_succeeds() {
        let p = MemoryProvider::new();
        p.write("z", b"1", WritePrecondition::CreateNew).unwrap();
        // Unconditional 删除已存在对象，不检查版本。
        p.delete("z", DeletePrecondition::Unconditional).unwrap();
        assert!(p.read("z").unwrap().is_none());
    }

    #[test]
    fn unconditional_delete_missing_is_idempotent() {
        let p = MemoryProvider::new();
        // Unconditional 删除不存在的对象 = 幂等成功（无需删除）。
        p.delete("missing", DeletePrecondition::Unconditional)
            .unwrap();
        assert!(p.read("missing").unwrap().is_none());
    }

    #[test]
    fn unconditional_write_does_not_reject_on_version_change() {
        // 与 IfMatch 对比：IfMatch 用旧版本会失败，Unconditional 总是成功。
        let p = MemoryProvider::new();
        let v1 = p.write("k", b"1", WritePrecondition::CreateNew).unwrap();
        let _v2 = p
            .write("k", b"2", WritePrecondition::IfMatch(v1.clone()))
            .unwrap();
        // 此时远端版本已是 _v2，用 v1 走 IfMatch 会失败，但 Unconditional 应成功。
        p.write("k", b"3", WritePrecondition::Unconditional)
            .unwrap();
        let obj = p.read("k").unwrap().unwrap();
        assert_eq!(obj.content, b"3");
    }

    #[test]
    fn create_new_does_not_query_old_sha_and_rejects_existing() {
        // CreateNew 语义：不查旧 SHA；对象已存在直接返回 PreconditionFailed。
        let p = MemoryProvider::new();
        p.write("c", b"1", WritePrecondition::CreateNew).unwrap();
        let err = p
            .write("c", b"2", WritePrecondition::CreateNew)
            .unwrap_err();
        assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
    }

    #[test]
    fn if_match_does_not_refresh_sha_on_conflict() {
        // IfMatch 语义：严格使用调用方给的版本，冲突时返回 PreconditionFailed，
        // 绝不刷新 SHA 后继续写。这里用旧版本连续两次写入都应失败。
        let p = MemoryProvider::new();
        let v1 = p.write("m", b"1", WritePrecondition::CreateNew).unwrap();
        let _v2 = p
            .write("m", b"2", WritePrecondition::IfMatch(v1.clone()))
            .unwrap();
        let err1 = p
            .write("m", b"3", WritePrecondition::IfMatch(v1.clone()))
            .unwrap_err();
        assert!(matches!(err1, ProviderError::PreconditionFailed { .. }));
        // 再次用同一旧版本尝试，仍应失败（没有"刷新 SHA 重试"的旁路）。
        let err2 = p
            .write("m", b"4", WritePrecondition::IfMatch(v1))
            .unwrap_err();
        assert!(matches!(err2, ProviderError::PreconditionFailed { .. }));
        // 远端内容仍是 _v2 对应的 b"2"，未被旧版本写入改写。
        let obj = p.read("m").unwrap().unwrap();
        assert_eq!(obj.content, b"2");
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
