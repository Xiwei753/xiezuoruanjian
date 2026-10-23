//! 通用 Git HTTPS Provider — 标准 Git Smart HTTP，不调用任何平台 REST API。
//!
//! 支持 GitLab、Gitee、Gitea、Forgejo、Bitbucket 与自建 Git 服务：
//! 只使用仓库地址、分支、可选用户名与密码/访问令牌，由远端服务器判定凭据。
//!
//! 远端版本 = Git blob OID；`commit_batch` 在本地构造完整 tree 与一个 commit，
//! 再执行普通（非强制）push。远端变化导致 non-fast-forward 时返回
//! `PreconditionFailed`，交给现有 LWW 重新读取并决策。

pub mod config;
mod credentials;
mod repository;

use std::path::PathBuf;
use std::sync::Mutex;

use crate::sync::provider::capabilities::SyncCapabilities;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion,
    WritePrecondition,
};
use crate::sync::provider::SyncProvider;
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::url::sanitize_remote_url;

pub use config::GitHttpsProviderConfig;
pub use config::GitHttpsRuntimeConfig;

/// 通用 Git HTTPS Provider。
pub struct GitHttpsProvider {
    config: GitHttpsRuntimeConfig,
    cache_dir: PathBuf,
    snapshot: Mutex<Option<repository::Snapshot>>,
}

impl GitHttpsProvider {
    pub fn new(config: GitHttpsRuntimeConfig, cache_dir: PathBuf) -> Self {
        Self {
            config,
            cache_dir,
            snapshot: Mutex::new(None),
        }
    }

    pub fn config(&self) -> &GitHttpsRuntimeConfig {
        &self.config
    }

    /// 只连接并读取 refs，用于路由探测与诊断。
    pub(crate) fn probe(&self) -> Result<(), ProviderError> {
        repository::probe(&self.cache_dir, &self.config)
    }

    /// 返回当前快照；`force` 为 true 时重新 fetch。
    fn snapshot(&self, force: bool) -> Result<repository::Snapshot, ProviderError> {
        let mut guard = self.snapshot.lock().map_err(|_| ProviderError::Other {
            reason: "git provider snapshot lock poisoned".to_string(),
        })?;
        if !force {
            if let Some(snapshot) = guard.as_ref() {
                return Ok(snapshot.clone());
            }
        }
        let repo = repository::open_or_init(&self.cache_dir)?;
        let snapshot = repository::fetch_snapshot(&repo, &self.config)?;
        *guard = Some(snapshot.clone());
        Ok(snapshot)
    }

    fn invalidate(&self) {
        if let Ok(mut guard) = self.snapshot.lock() {
            *guard = None;
        }
    }

    fn commit(
        &self,
        mutations: &[BatchMutation],
        message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        if mutations.is_empty() {
            return Ok(BatchCommitResult {
                revision: RemoteVersion::new(""),
                touched_paths: Vec::new(),
            });
        }
        let repo = repository::open_or_init(&self.cache_dir)?;
        // 提交前强制重新读取远端 HEAD，避免沿用旧前置条件。
        let snapshot = repository::fetch_snapshot(&repo, &self.config)?;
        let commit_oid =
            repository::commit_and_push(&repo, &self.config, &snapshot, mutations, message)?;
        self.invalidate();
        Ok(BatchCommitResult {
            revision: RemoteVersion::new(commit_oid.to_string()),
            touched_paths: mutations.iter().map(|m| m.path().to_string()).collect(),
        })
    }
}

impl SyncProvider for GitHttpsProvider {
    fn capabilities(&self) -> SyncCapabilities {
        SyncCapabilities::git_https()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let snapshot = self.snapshot(false)?;
        let needle = if prefix.is_empty() {
            None
        } else {
            Some(format!("{prefix}/"))
        };
        let mut entries = Vec::new();
        for (path, oid) in snapshot.blobs.iter() {
            let entry_path = match &needle {
                None => Some(path.clone()),
                Some(needle) => path.strip_prefix(needle).map(str::to_string),
            };
            if let Some(path) = entry_path {
                entries.push(RemoteEntry {
                    path,
                    version: RemoteVersion::new(oid.to_string()),
                });
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        let snapshot = self.snapshot(false)?;
        if !snapshot.blobs.contains_key(path) {
            // 可能被其他设备刚写入；刷新一次再判断。
            let snapshot = self.snapshot(true)?;
            if !snapshot.blobs.contains_key(path) {
                return Ok(None);
            }
            let repo = repository::open_or_init(&self.cache_dir)?;
            let content = repository::read_blob(&repo, &snapshot, path)?;
            return Ok(content.map(|content| RemoteObject {
                path: path.to_string(),
                content,
                version: repository::blob_version(&snapshot, path)
                    .unwrap_or_else(|| RemoteVersion::new("")),
            }));
        }
        let repo = repository::open_or_init(&self.cache_dir)?;
        let content = repository::read_blob(&repo, &snapshot, path)?;
        Ok(content.map(|content| RemoteObject {
            path: path.to_string(),
            content,
            version: repository::blob_version(&snapshot, path)
                .unwrap_or_else(|| RemoteVersion::new("")),
        }))
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        // 单对象写入必须先读取最新远端状态，再在最新快照上校验前置条件。
        let snapshot = self.snapshot(true)?;
        repository::check_write_precondition(&snapshot, path, &precondition)?;
        let version = RemoteVersion::new(
            git2::Oid::hash_object(git2::ObjectType::Blob, content)
                .map_err(|err| repository::map_git_error(err, "hash blob"))?
                .to_string(),
        );
        self.commit(
            &[BatchMutation::Put {
                path: path.to_string(),
                content: content.to_vec(),
            }],
            &format!("sujian: write {path}"),
        )?;
        Ok(version)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        let snapshot = self.snapshot(true)?;
        repository::check_delete_precondition(&snapshot, path, &precondition)?;
        if !snapshot.blobs.contains_key(path) {
            return Ok(());
        }
        self.commit(
            &[BatchMutation::Delete {
                path: path.to_string(),
            }],
            &format!("sujian: delete {path}"),
        )?;
        Ok(())
    }

    fn commit_batch(
        &self,
        mutations: &[BatchMutation],
        message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        self.commit(mutations, message)
    }

    fn diagnose(&self) -> Result<SyncDiagnosticsResult, ProviderError> {
        let mut result = SyncDiagnosticsResult::new();
        result.provider_type = "git_https".to_string();
        result.has_network_permission = true;
        result.has_network_state_permission = true;
        result.network_state = "unchecked".to_string();
        result.provider_details = Some(format!(
            "remote_url: {}, branch: {}",
            sanitize_remote_url(&self.config.remote_url).sanitized_url,
            self.config.branch
        ));

        match repository::probe(&self.cache_dir, &self.config) {
            Ok(()) => {
                result.success = true;
                result.network_ok = true;
                result.remote_ok = true;
                result.auth_ok = self.config.secret.is_some();
                result.network_status = "ok".to_string();
                result.auth_status = if self.config.secret.is_some() {
                    "ok".to_string()
                } else {
                    "anonymous".to_string()
                };
            }
            Err(error) => {
                result.success = false;
                result.error_category = error.to_sync_error_category().to_ui_status().to_string();
                result.network_ok = matches!(error, ProviderError::Network { .. });
                result.auth_ok = !matches!(
                    error,
                    ProviderError::AuthFailed { .. } | ProviderError::PermissionDenied { .. }
                );
                result.remote_ok = false;
                result.network_status = if result.network_ok {
                    "unreachable".to_string()
                } else {
                    "checked".to_string()
                };
                result.auth_status = if result.auth_ok {
                    "checked".to_string()
                } else {
                    "failed".to_string()
                };
                result.raw_error = Some(crate::sync::url::redact_secrets_from_message(
                    &error.to_string(),
                    self.config.secret.as_deref(),
                    Some(&self.config.remote_url),
                ));
            }
        }
        Ok(result)
    }
}
