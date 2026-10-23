//! Peer Provider — 附近设备同步的隔离远端镜像（Issue #988）。
//!
//! ArkTS 把对端对象写进本次会话的临时目录，Core 只把这个目录当作**远端**读取；
//! 下载内容仍走既有 staging → LWW → commit 进入 live 数据根，绝不直接覆盖 live。
//! Core 在同一轮里写回该目录的对象就是要发回对端的变更，同步结束后通过
//! [`PeerProvider::drain_mutations`] 取出，由 ArkTS 按文本分片送回。
//!
//! 版本使用内容 SHA-256：同一内容重发得到同一版本，If-Match 可以在重试下幂等。
//! 该 Provider 不支持批量原子提交，引擎会逐对象写删。

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use super::capabilities::SyncCapabilities;
use super::error::ProviderError;
use super::model::{
    DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion, WritePrecondition,
};
use super::SyncProvider;

/// Core 对本会话远端镜像造成的变更。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerMutation {
    Put {
        path: String,
        content: Vec<u8>,
        version: String,
    },
    Delete {
        path: String,
    },
}

/// 文件系统实现的 Peer 远端镜像。
pub struct PeerProvider {
    root: PathBuf,
    mutations: Mutex<Vec<PeerMutation>>,
}

impl PeerProvider {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ProviderError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|err| ProviderError::Other {
            reason: format!("create peer session root failed: {err}"),
        })?;
        Ok(Self {
            root,
            mutations: Mutex::new(Vec::new()),
        })
    }

    /// 取走本轮 Core 写回镜像的变更，供 ArkTS 发回对端。
    pub fn drain_mutations(&self) -> Vec<PeerMutation> {
        match self.mutations.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }

    fn lock_mutations(&self) -> std::sync::MutexGuard<'_, Vec<PeerMutation>> {
        self.mutations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn safe_path(&self, path: &str) -> Result<PathBuf, ProviderError> {
        if path.is_empty() || path.contains('\\') {
            return Err(ProviderError::Other {
                reason: format!("invalid peer object path: {path}"),
            });
        }
        let relative = Path::new(path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(ProviderError::Other {
                reason: format!("unsafe peer object path: {path}"),
            });
        }
        Ok(self.root.join(relative))
    }

    fn version_for(content: &[u8]) -> RemoteVersion {
        let digest = Sha256::digest(content);
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(&mut hex, "{byte:02x}");
        }
        RemoteVersion(hex)
    }

    fn read_entry(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        let full = self.safe_path(path)?;
        match fs::read(&full) {
            Ok(content) => Ok(Some(RemoteObject {
                path: path.to_string(),
                version: Self::version_for(&content),
                content,
            })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(ProviderError::Other {
                reason: format!("read peer object {path} failed: {err}"),
            }),
        }
    }

    fn collect(
        &self,
        directory: &Path,
        prefix: &str,
        output: &mut Vec<RemoteEntry>,
    ) -> Result<(), ProviderError> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(ProviderError::Other {
                    reason: format!("list peer directory failed: {err}"),
                })
            }
        };
        for entry in entries {
            let entry = entry.map_err(|err| ProviderError::Other {
                reason: format!("list peer directory entry failed: {err}"),
            })?;
            let file_type = entry.file_type().map_err(|err| ProviderError::Other {
                reason: format!("peer entry type failed: {err}"),
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if file_type.is_dir() {
                self.collect(&entry.path(), &relative, output)?;
            } else if file_type.is_file() {
                let content = fs::read(entry.path()).map_err(|err| ProviderError::Other {
                    reason: format!("read peer object {relative} failed: {err}"),
                })?;
                output.push(RemoteEntry {
                    path: relative,
                    version: Self::version_for(&content),
                });
            }
        }
        Ok(())
    }

    fn write_content(&self, path: &str, content: &[u8]) -> Result<RemoteVersion, ProviderError> {
        let full = self.safe_path(path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|err| ProviderError::Other {
                reason: format!("create peer parent dir failed: {err}"),
            })?;
        }
        let temp = full.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        {
            let mut file = fs::File::create(&temp).map_err(|err| ProviderError::Other {
                reason: format!("create peer temp object failed: {err}"),
            })?;
            file.write_all(content).map_err(|err| ProviderError::Other {
                reason: format!("write peer object failed: {err}"),
            })?;
            file.sync_all().map_err(|err| ProviderError::Other {
                reason: format!("sync peer object failed: {err}"),
            })?;
        }
        fs::rename(&temp, &full).map_err(|err| ProviderError::Other {
            reason: format!("publish peer object failed: {err}"),
        })?;
        let version = Self::version_for(content);
        self.lock_mutations().push(PeerMutation::Put {
            path: path.to_string(),
            content: content.to_vec(),
            version: version.0.clone(),
        });
        Ok(version)
    }
}

impl SyncProvider for PeerProvider {
    fn capabilities(&self) -> SyncCapabilities {
        SyncCapabilities {
            conditional_write: true,
            atomic_write: false,
            atomic_move: false,
            batch: false,
            server_timestamp: false,
            directory_semantics: false,
            remote_history: false,
            max_parallel_downloads: 1,
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let mut entries = Vec::new();
        self.collect(&self.root, "", &mut entries)?;
        let needle = if prefix.is_empty() {
            None
        } else {
            Some(format!("{prefix}/"))
        };
        let mut filtered = Vec::new();
        for entry in entries {
            match &needle {
                None => filtered.push(entry),
                Some(needle) => {
                    if let Some(stripped) = entry.path.strip_prefix(needle) {
                        filtered.push(RemoteEntry {
                            path: stripped.to_string(),
                            version: entry.version,
                        });
                    }
                }
            }
        }
        filtered.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(filtered)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        self.read_entry(path)
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        let current = self.read_entry(path)?;
        match precondition {
            WritePrecondition::IfMatch(expected) => match current {
                Some(object) if object.version == expected => {}
                Some(object) => {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!(
                            "version mismatch: expected={}, current={}",
                            expected.0, object.version.0
                        ),
                    })
                }
                None => {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: "object does not exist".to_string(),
                    })
                }
            },
            WritePrecondition::CreateNew => {
                if current.is_some() {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: "object already exists".to_string(),
                    });
                }
            }
            WritePrecondition::Unconditional => {}
        }
        self.write_content(path, content)
    }

    fn delete(
        &self,
        path: &str,
        precondition: DeletePrecondition,
    ) -> Result<(), ProviderError> {
        let current = self.read_entry(path)?;
        match precondition {
            DeletePrecondition::IfMatch(expected) => match &current {
                Some(object) if object.version == expected => {}
                Some(object) => {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!(
                            "version mismatch: expected={}, current={}",
                            expected.0, object.version.0
                        ),
                    })
                }
                None => {
                    return Err(ProviderError::NotFound {
                        path: path.to_string(),
                    })
                }
            },
            DeletePrecondition::Unconditional => {}
        }
        if current.is_none() {
            return Ok(());
        }
        let full = self.safe_path(path)?;
        fs::remove_file(&full).map_err(|err| ProviderError::Other {
            reason: format!("delete peer object {path} failed: {err}"),
        })?;
        self.lock_mutations().push(PeerMutation::Delete {
            path: path.to_string(),
        });
        Ok(())
    }
}

/// 本轮从对端收到、已写入会话镜像的对象路径（供诊断/幂等确认）。
pub fn snapshot_versions(root: &Path) -> Result<HashMap<String, String>, ProviderError> {
    let provider = PeerProvider::new(root)?;
    let mut versions = HashMap::new();
    for entry in provider.list("")? {
        versions.insert(entry.path, entry.version.0);
    }
    Ok(versions)
}
