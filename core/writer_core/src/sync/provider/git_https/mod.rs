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
use std::sync::{Arc, Mutex};

use crate::sync::provider::capabilities::SyncCapabilities;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion,
    WritePrecondition,
};
use crate::sync::provider::{ProviderNetworkAccounting, ProviderNetworkMetrics, SyncProvider};
use crate::sync::types::SyncDiagnosticsResult;
use crate::sync::url::sanitize_remote_url;

pub use config::GitHttpsProviderConfig;
pub use config::GitHttpsRuntimeConfig;

const GIT_HTTPS_METRIC_SCOPE: &str = "git_https_provider_operations_pack_bytes_lower_bound";

/// 通用 Git HTTPS Provider。
pub struct GitHttpsProvider {
    config: GitHttpsRuntimeConfig,
    cache_dir: PathBuf,
    snapshot: Mutex<Option<repository::Snapshot>>,
    network_accounting: Arc<ProviderNetworkAccounting>,
}

impl GitHttpsProvider {
    pub fn new(config: GitHttpsRuntimeConfig, cache_dir: PathBuf) -> Self {
        Self {
            config,
            cache_dir,
            snapshot: Mutex::new(None),
            network_accounting: Arc::new(ProviderNetworkAccounting::new(GIT_HTTPS_METRIC_SCOPE)),
        }
    }

    pub fn config(&self) -> &GitHttpsRuntimeConfig {
        &self.config
    }

    /// 只连接并读取 refs，用于路由探测与诊断。
    pub(crate) fn probe(&self) -> Result<(), ProviderError> {
        repository::probe(&self.cache_dir, &self.config, &self.network_accounting)
    }

    /// 返回本轮同步固定使用的远端快照；首次调用时 fetch，之后一直复用。
    fn snapshot(&self) -> Result<repository::Snapshot, ProviderError> {
        let mut guard = self.snapshot.lock().map_err(|_| ProviderError::Other {
            reason: "git provider snapshot lock poisoned".to_string(),
        })?;
        if let Some(snapshot) = guard.as_ref() {
            return Ok(snapshot.clone());
        }
        let repo = repository::open_or_init(&self.cache_dir)?;
        let snapshot = repository::fetch_snapshot(&repo, &self.config, &self.network_accounting)?;
        *guard = Some(snapshot.clone());
        Ok(snapshot)
    }

    /// 仅当缓存仍是本次提交的规划基线时，推进到自己的新提交。
    ///
    /// 绝不把远端 fetch 到的新 HEAD 当作下一轮规划基线；外部 HEAD 变化只能
    /// 通过 `commit_from` 的前置检查返回 `PreconditionFailed`，由外层新建
    /// provider 重新做完整 LWW。
    fn advance_snapshot(&self, planned: &repository::Snapshot, committed: repository::Snapshot) {
        if let Ok(mut guard) = self.snapshot.lock() {
            if guard.as_ref().map(|snapshot| snapshot.head) == Some(planned.head) {
                *guard = Some(committed);
            }
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
        let planned = self.snapshot()?;
        self.commit_from(planned, mutations, message)
    }

    /// 以 `planned` 为唯一基线提交。
    ///
    /// 本轮已经 fetch 过一次远端分支；提交时不再 fetch。普通（非强制）push 的
    /// fast-forward 约束负责拒绝远端新提交，返回 `PreconditionFailed` 后由外层
    /// 新建 provider、重新读取 catalog 并重新做完整 LWW，绝不重放旧结果。
    fn commit_from(
        &self,
        planned: repository::Snapshot,
        mutations: &[BatchMutation],
        message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        let repo = repository::open_or_init(&self.cache_dir)?;
        let (commit_oid, committed_snapshot) = repository::commit_and_push(
            &repo,
            &self.config,
            &planned,
            mutations,
            message,
            &self.network_accounting,
        )?;
        self.advance_snapshot(&planned, committed_snapshot);
        Ok(BatchCommitResult {
            revision: RemoteVersion::new(commit_oid.to_string()),
            touched_paths: mutations
                .iter()
                .filter(|mutation| {
                    !matches!(
                        mutation,
                        BatchMutation::CheckObject { .. } | BatchMutation::CheckPrefix { .. }
                    )
                })
                .map(|mutation| mutation.path().to_string())
                .collect(),
        })
    }
}

impl SyncProvider for GitHttpsProvider {
    fn set_operation_id(&self, operation_id: Option<&str>) {
        self.network_accounting.set_operation_id(operation_id);
    }

    fn network_metrics(&self) -> ProviderNetworkMetrics {
        self.network_accounting.snapshot()
    }

    fn capabilities(&self) -> SyncCapabilities {
        SyncCapabilities::git_https()
    }

    fn remote_revision(&self) -> Result<Option<String>, ProviderError> {
        // Reuse the round snapshot.  The first call fetches the branch once;
        // every later call (planning recheck, checkpoint, diagnostics) reads
        // the same fixed snapshot instead of opening another connection.
        Ok(self.snapshot()?.head.map(|head| head.to_string()))
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let snapshot = self.snapshot()?;
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
        // 本轮同步固定使用首次读取的 HEAD 快照；快照中没有的路径直接视为
        // 远端不存在。不能在这里隐式 fetch 并替换规划基线。
        let snapshot = self.snapshot()?;
        if !snapshot.blobs.contains_key(path) {
            return Ok(None);
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
        // 前置条件必须针对本轮计划使用的快照校验；commit 会再确认远端 HEAD 未变化。
        let snapshot = self.snapshot()?;
        repository::check_write_precondition(&snapshot, path, &precondition)?;
        let version = RemoteVersion::new(
            git2::Oid::hash_object(git2::ObjectType::Blob, content)
                .map_err(|err| repository::map_git_error(err, "hash_blob"))?
                .to_string(),
        );
        self.commit_from(
            snapshot,
            &[BatchMutation::Put {
                path: path.to_string(),
                content: content.to_vec(),
            }],
            &format!("sujian: write {path}"),
        )?;
        Ok(version)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        let snapshot = self.snapshot()?;
        repository::check_delete_precondition(&snapshot, path, &precondition)?;
        if !snapshot.blobs.contains_key(path) {
            return Ok(());
        }
        self.commit_from(
            snapshot,
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

        match repository::probe(&self.cache_dir, &self.config, &self.network_accounting) {
            Ok(()) => {
                result.success = true;
                result.network_ok = true;
                result.remote_ok = true;
                // 探测成功即说明匿名或认证访问均可建立；匿名仓库不能算认证失败。
                result.auth_ok = true;
                result.network_status = "ok".to_string();
                result.auth_status = if self.config.secret.is_some() {
                    "ok".to_string()
                } else {
                    "anonymous".to_string()
                };
            }
            Err(error) => {
                result.success = false;
                result.error_category = match &error {
                    ProviderError::GitRuntimeInit { .. }
                    | ProviderError::TlsCaUnavailable { .. }
                    | ProviderError::TlsCertificateInvalid { .. }
                    | ProviderError::LocalGitCache { .. }
                    | ProviderError::RemoteConfig { .. } => error.stable_code().to_string(),
                    _ => error.to_sync_error_category().to_ui_status().to_string(),
                };
                result.network_ok = !matches!(
                    error,
                    ProviderError::Network { .. }
                        | ProviderError::TemporaryUnavailable { .. }
                        | ProviderError::GitRuntimeInit { .. }
                        | ProviderError::TlsCaUnavailable { .. }
                        | ProviderError::TlsCertificateInvalid { .. }
                        | ProviderError::LocalGitCache { .. }
                        | ProviderError::RemoteConfig { .. }
                );
                result.auth_ok = !matches!(
                    error,
                    ProviderError::AuthFailed { .. } | ProviderError::PermissionDenied { .. }
                );
                result.remote_ok = false;
                result.network_status = if result.network_ok {
                    "checked".to_string()
                } else {
                    "unreachable".to_string()
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
