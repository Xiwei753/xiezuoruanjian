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
    BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion,
    WritePrecondition,
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

    /// MemoryProvider 批量原子提交 — 单锁内事务执行所有 mutation。
    ///
    /// 在同一锁内顺序应用 Put / ReuseVersion / Delete，任一 ReuseVersion
    /// 引用了不存在的远端版本则返回 `PreconditionFailed`，整个 batch 不生效
    /// （未提交的 mutation 不写入 store，已应用的 mutation 因锁尚未释放也不可见）。
    ///
    /// `revision` 为本次事务的 UUID（仅用于诊断/前置条件，不参与业务逻辑）。
    /// `touched_paths` 列出本次 batch 实际生效的路径。
    fn commit_batch(
        &self,
        mutations: &[BatchMutation],
        message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        let _ = message;
        let mut store = self.store.lock().map_err(|_| Self::lock_err())?;
        let mut touched: Vec<String> = Vec::with_capacity(mutations.len());
        let txn_revision = Self::new_version();
        for m in mutations {
            match m {
                BatchMutation::Put { path, content } => {
                    store.insert(path.clone(), (content.clone(), Self::new_version()));
                    touched.push(path.clone());
                }
                BatchMutation::ReuseVersion { path, version } => {
                    // 复用已有远端对象版本：在 MemoryProvider 语义下等价于
                    // 把已有 (content, version) 复制到目标 path。若 version 不存在
                    // 于当前 store，返回 PreconditionFailed（远端无此 blob 可复用）。
                    let content = Self::reuse_content(&store, path, version)?;
                    store.insert(path.clone(), (content, version.clone()));
                    touched.push(path.clone());
                }
                BatchMutation::Delete { path } => {
                    store.remove(path);
                    touched.push(path.clone());
                }
            }
        }
        Ok(BatchCommitResult {
            revision: txn_revision,
            touched_paths: touched,
        })
    }
}

impl MemoryProvider {
    /// 在 store 中查找 version 对应的 content，返回复用内容。
    ///
    /// 找到 → `Ok(content)`；找不到 → `Err(PreconditionFailed)`。
    /// 抽出来避免 `commit_batch` 嵌套过深 / 类型过复杂。
    fn reuse_content(
        store: &Store,
        target_path: &str,
        version: &RemoteVersion,
    ) -> Result<Vec<u8>, ProviderError> {
        store
            .iter()
            .find(|(_, (_, v))| *v == *version)
            .map(|(_, (c, _))| c.clone())
            .ok_or_else(|| ProviderError::PreconditionFailed {
                path: target_path.to_string(),
                reason: format!("reuse_version: remote version {version} not found"),
            })
    }
}

#[cfg(test)]
mod tests;
