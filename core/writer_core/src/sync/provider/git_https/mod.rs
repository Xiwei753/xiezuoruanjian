//! 通用 Git HTTPS Provider — 标准 Git Smart HTTP，不调用任何平台 REST API。
//!
//! 支持 GitLab、Gitee、Gitea、Forgejo、Bitbucket 与自建 Git 服务：
//! 只使用仓库地址、分支、可选用户名与密码/访问令牌，由远端服务器判定凭据。
//!
//! 远端版本 = Git blob OID；`commit_batch` 以固定 base tree OID + 本轮变更只重建
//! 受影响目录，构造一个 commit 后执行普通（非强制）push。远端变化导致
//! non-fast-forward 时返回 `PreconditionFailed`，交给现有 LWW 重新读取并决策。

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
                path_versions: std::collections::BTreeMap::new(),
            });
        }
        let planned = self.snapshot()?;
        let repo = repository::open_or_init(&self.cache_dir)?;
        self.commit_from(&repo, planned, mutations, message)
    }

    /// 以 `planned` 为唯一基线提交。
    ///
    /// 本轮已经 fetch 过一次远端分支；提交时不再 fetch。普通（非强制）push 的
    /// fast-forward 约束负责拒绝远端新提交，返回 `PreconditionFailed` 后由外层
    /// 新建 provider、重新读取 catalog 并重新做完整 LWW，绝不重放旧结果。
    fn commit_from(
        &self,
        repo: &git2::Repository,
        planned: repository::Snapshot,
        mutations: &[BatchMutation],
        message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        let (commit_oid, committed_snapshot) = repository::commit_and_push(
            repo,
            &self.config,
            &planned,
            mutations,
            message,
            &self.network_accounting,
        )?;
        // 用提交后的实际 tree 证明每个对象写成了什么：Git OID 与决议内容
        // 不一致时调用方可以据此拒绝确认（评审 #7214314073）。
        let mut path_versions = std::collections::BTreeMap::new();
        for mutation in mutations {
            if let BatchMutation::Put { path, .. } | BatchMutation::ReuseVersion { path, .. } =
                mutation
            {
                if let Some(version) = repository::blob_version(repo, &committed_snapshot, path)? {
                    path_versions.insert(path.clone(), version);
                }
            }
        }
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
            path_versions,
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

    /// 只读远端 refs 的轻量探测。空闲轮询用它把远端 HEAD 与上次消费的提交
    /// 比较，不 fetch、不 flatten tree；HEAD 没变就直接结束本轮。
    fn probe_remote_revision(&self) -> Result<Option<String>, ProviderError> {
        repository::probe_head(&self.cache_dir, &self.config, &self.network_accounting)
            .map(|head| head.map(|oid| oid.to_string()))
    }

    /// 以固定快照为 head、用本地缓存的 commit 做 tree diff。
    ///
    /// 只有当 `base_revision` 对应提交仍在本地缓存时才能给出差异；否则返回
    /// `None`，由调用方回到完整扫描（迁移/浅克隆/缓存被清理）。
    fn changed_paths_since(
        &self,
        base_revision: &str,
    ) -> Result<Option<Vec<String>>, ProviderError> {
        let snapshot = self.snapshot()?;
        let Some(head) = snapshot.head else {
            return Ok(None);
        };
        let repo = repository::open_or_init(&self.cache_dir)?;
        repository::changed_paths_between(&repo, base_revision, head)
    }

    /// HEAD 已确认未变时，用本地缓存提交构造快照，省掉一次 fetch。
    fn warm_snapshot_from_cache(&self, revision: &str) -> bool {
        let Ok(mut guard) = self.snapshot.lock() else {
            return false;
        };
        if guard.is_some() {
            return true;
        }
        let Ok(oid) = git2::Oid::from_str(revision) else {
            return false;
        };
        let Ok(repo) = repository::open_or_init(&self.cache_dir) else {
            return false;
        };
        match repository::snapshot_from_commit(&repo, oid) {
            Ok(snapshot) => {
                *guard = Some(snapshot);
                true
            }
            Err(_) => false,
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let snapshot = self.snapshot()?;
        let repo = repository::open_or_init(&self.cache_dir)?;
        let needle = if prefix.is_empty() {
            None
        } else {
            Some(format!("{prefix}/"))
        };
        let mut entries = Vec::new();
        for (path, oid) in snapshot.list_blobs(&repo)? {
            let entry_path = match &needle {
                None => Some(path),
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
        let repo = repository::open_or_init(&self.cache_dir)?;
        let Some(content) = repository::read_blob(&repo, &snapshot, path)? else {
            return Ok(None);
        };
        let version = repository::blob_version(&repo, &snapshot, path)?
            .unwrap_or_else(|| RemoteVersion::new(""));
        Ok(Some(RemoteObject {
            path: path.to_string(),
            content,
            version,
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
        let repo = repository::open_or_init(&self.cache_dir)?;
        repository::check_write_precondition(&repo, &snapshot, path, &precondition)?;
        let version = RemoteVersion::new(
            git2::Oid::hash_object(git2::ObjectType::Blob, content)
                .map_err(|err| repository::map_git_error(err, "hash_blob"))?
                .to_string(),
        );
        self.commit_from(
            &repo,
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
        let repo = repository::open_or_init(&self.cache_dir)?;
        repository::check_delete_precondition(&repo, &snapshot, path, &precondition)?;
        if snapshot.blob_oid(&repo, path)?.is_none() {
            return Ok(());
        }
        self.commit_from(
            &repo,
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
