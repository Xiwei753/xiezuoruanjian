//! One-round deferred provider writes.
//!
//! A full sync round reads one fixed remote snapshot, decides every object with
//! LWW, and publishes the complete result once.  This wrapper collects the
//! provider writes performed while executing the planned targets and emits a
//! single `commit_batch` (or a sequential fallback for non-batch providers) at
//! the end of the round.
//!
//! Reads and `list` calls see the buffered overlay, so the catalog CAS helpers
//! keep working against the latest in-round state without network round trips.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use crate::sync::provider::capabilities::SyncCapabilities;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion,
    WritePrecondition,
};
use crate::sync::provider::{ProviderNetworkMetrics, SyncProvider};

#[derive(Clone)]
enum BufferedValue {
    Content(Vec<u8>),
    Version(RemoteVersion),
}

struct RoundState {
    entries: BTreeMap<String, Option<BufferedValue>>,
    /// Real remote version observed on first touch, used for the final
    /// baseline check.  `None` means the object did not exist.
    baselines: BTreeMap<String, Option<RemoteVersion>>,
    /// Paths whose write/delete precondition was validated against the in-round
    /// overlay.  `flush` re-checks them against the real remote baseline.
    preconditioned_paths: BTreeSet<String>,
    /// Conditions that must be checked against the real remote baseline by the
    /// final commit, not against the in-round overlay.
    checks: Vec<BatchMutation>,
    /// Full remote tree cached once for the round.  Prevents GitHub's
    /// `list(prefix)` (which fetches the recursive tree) from being called
    /// again for every prefix/blob reuse.
    base_entries: Option<Vec<RemoteEntry>>,
}

/// Result of the single remote publication at the end of a round.
pub struct RoundFlushOutcome {
    /// Whether a commit/tree change was actually published.
    pub committed: bool,
    pub result: BatchCommitResult,
}

/// 本轮即将写入远端的一条对象事实（发布回执的输入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReceipt {
    /// 远端绝对路径（含 target 前缀）。
    pub path: String,
    /// `upsert` 或 `delete`。
    pub op: String,
    /// upsert 的实际内容摘要（MD5）；`ReuseVersion` 时为复用的远端版本串。
    pub content_digest: Option<String>,
}

/// Provider wrapper that defers all mutations until [`Self::flush`].
pub struct RoundProvider<'a> {
    inner: &'a dyn SyncProvider,
    base_revision: Option<RemoteVersion>,
    state: Mutex<RoundState>,
}

impl<'a> RoundProvider<'a> {
    pub fn new(inner: &'a dyn SyncProvider, base_revision: Option<RemoteVersion>) -> Self {
        Self {
            inner,
            base_revision,
            state: Mutex::new(RoundState {
                entries: BTreeMap::new(),
                baselines: BTreeMap::new(),
                preconditioned_paths: BTreeSet::new(),
                checks: Vec::new(),
                base_entries: None,
            }),
        }
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, RoundState>, ProviderError> {
        self.state.lock().map_err(|_| ProviderError::Other {
            reason: "sync round buffer lock poisoned".to_string(),
        })
    }

    fn buffered_version(content: &[u8]) -> RemoteVersion {
        RemoteVersion::new(format!("round:{}", crate::sync::hash::content_md5(content)))
    }

    fn buffered_version_for(entry: &Option<BufferedValue>) -> Option<RemoteVersion> {
        entry.as_ref().map(|value| match value {
            BufferedValue::Content(content) => Self::buffered_version(content),
            BufferedValue::Version(version) => version.clone(),
        })
    }

    fn real_baseline_for(&self, path: &str) -> Result<Option<RemoteVersion>, ProviderError> {
        {
            let state = self.lock_state()?;
            if let Some(version) = state.baselines.get(path) {
                return Ok(version.clone());
            }
        }
        let version = self.inner.read(path)?.map(|object| object.version);
        let mut state = self.lock_state()?;
        Ok(state
            .baselines
            .entry(path.to_string())
            .or_insert_with(|| version)
            .clone())
    }

    fn current_version(&self, path: &str) -> Result<Option<RemoteVersion>, ProviderError> {
        let buffered = {
            let state = self.lock_state()?;
            if let Some(entry) = state.entries.get(path) {
                Some(Self::buffered_version_for(entry))
            } else {
                None
            }
        };
        if let Some(version) = buffered {
            return Ok(version);
        }
        self.real_baseline_for(path)
    }

    /// Validate a write precondition against the in-round overlay and remember
    /// that `flush` must re-check the same path against the real baseline.
    fn check_write_precondition(
        &self,
        path: &str,
        precondition: &WritePrecondition,
    ) -> Result<(), ProviderError> {
        match precondition {
            WritePrecondition::Unconditional => return Ok(()),
            WritePrecondition::CreateNew => {
                if self.current_version(path)?.is_some() {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: "object already exists".to_string(),
                    });
                }
            }
            WritePrecondition::IfMatch(expected) => {
                let actual = self.current_version(path)?;
                if actual.as_ref() != Some(expected) {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!(
                            "version mismatch: expected={}, current={:?}",
                            expected.as_str(),
                            actual.as_ref().map(RemoteVersion::as_str)
                        ),
                    });
                }
            }
        }
        self.lock_state()?
            .preconditioned_paths
            .insert(path.to_string());
        Ok(())
    }

    fn check_delete_precondition(
        &self,
        path: &str,
        precondition: &DeletePrecondition,
    ) -> Result<(), ProviderError> {
        match precondition {
            DeletePrecondition::Unconditional => return Ok(()),
            DeletePrecondition::IfMatch(expected) => {
                let actual = self.current_version(path)?;
                if actual.as_ref() != Some(expected) {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!(
                            "version mismatch: expected={}, current={:?}",
                            expected.as_str(),
                            actual.as_ref().map(RemoteVersion::as_str)
                        ),
                    });
                }
            }
        }
        self.lock_state()?
            .preconditioned_paths
            .insert(path.to_string());
        Ok(())
    }

    /// Replace in-round virtual versions (`round:...`) with the real remote
    /// baseline before a check reaches the final provider.  A virtual version
    /// is only meaningful for the overlay; it is never a Git blob SHA.
    fn sanitize_check(
        &self,
        check: &BatchMutation,
        baselines: &BTreeMap<String, Option<RemoteVersion>>,
    ) -> Result<Option<BatchMutation>, ProviderError> {
        match check {
            BatchMutation::CheckObject {
                path,
                expected_version,
            } => {
                let expected_version = match expected_version {
                    Some(version) if Self::is_round_version(version) => {
                        self.baseline_from(baselines, path)?
                    }
                    other => other.clone(),
                };
                Ok(Some(BatchMutation::CheckObject {
                    path: path.clone(),
                    expected_version,
                }))
            }
            BatchMutation::CheckPrefix {
                prefix,
                expected_entries,
            } => {
                let needle = if prefix.is_empty() {
                    String::new()
                } else {
                    format!("{prefix}/")
                };
                let mut sanitized = Vec::with_capacity(expected_entries.len());
                for entry in expected_entries {
                    if Self::is_round_version(&entry.version) {
                        let full_path = format!("{needle}{}", entry.path);
                        // The path only exists in the overlay, so the real
                        // baseline tree does not contain it.
                        let Some(version) = self.baseline_from(baselines, &full_path)? else {
                            continue;
                        };
                        sanitized.push(RemoteEntry {
                            path: entry.path.clone(),
                            version,
                        });
                    } else {
                        sanitized.push(entry.clone());
                    }
                }
                Ok(Some(BatchMutation::CheckPrefix {
                    prefix: prefix.clone(),
                    expected_entries: sanitized,
                }))
            }
            other => Ok(Some(other.clone())),
        }
    }

    fn baseline_from(
        &self,
        baselines: &BTreeMap<String, Option<RemoteVersion>>,
        path: &str,
    ) -> Result<Option<RemoteVersion>, ProviderError> {
        if let Some(version) = baselines.get(path) {
            return Ok(version.clone());
        }
        self.real_baseline_for(path)
    }

    fn is_round_version(version: &RemoteVersion) -> bool {
        version.as_str().starts_with("round:")
    }

    fn all_remote_entries(&self) -> Result<Vec<RemoteEntry>, ProviderError> {
        {
            let state = self.lock_state()?;
            if let Some(entries) = &state.base_entries {
                return Ok(entries.clone());
            }
        }
        let entries = self.inner.list("")?;
        let mut state = self.lock_state()?;
        state.base_entries = Some(entries.clone());
        Ok(entries)
    }

    fn overlay_entries(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        let needle = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut entries: BTreeMap<String, RemoteVersion> = self
            .all_remote_entries()?
            .into_iter()
            .filter_map(|entry| {
                if needle.is_empty() {
                    Some((entry.path, entry.version))
                } else {
                    entry
                        .path
                        .strip_prefix(&needle)
                        .map(|path| (path.to_string(), entry.version))
                }
            })
            .collect();
        let state = self.lock_state()?;
        for (path, entry) in &state.entries {
            let relative = if needle.is_empty() {
                path.as_str()
            } else if let Some(relative) = path.strip_prefix(&needle) {
                relative
            } else {
                continue;
            };
            match entry {
                Some(value) => {
                    let version = match value {
                        BufferedValue::Content(content) => Self::buffered_version(content),
                        BufferedValue::Version(version) => version.clone(),
                    };
                    entries.insert(relative.to_string(), version);
                }
                None => {
                    entries.remove(relative);
                }
            }
        }
        Ok(entries
            .into_iter()
            .map(|(path, version)| RemoteEntry { path, version })
            .collect())
    }

    fn apply_mutation(&self, mutation: &BatchMutation) -> Result<(), ProviderError> {
        match mutation {
            BatchMutation::Put { path, content } => {
                let mut state = self.lock_state()?;
                state
                    .entries
                    .insert(path.clone(), Some(BufferedValue::Content(content.clone())));
                Ok(())
            }
            BatchMutation::ReuseVersion { path, version } => {
                let mut state = self.lock_state()?;
                state
                    .entries
                    .insert(path.clone(), Some(BufferedValue::Version(version.clone())));
                Ok(())
            }
            BatchMutation::Delete { path } => {
                let mut state = self.lock_state()?;
                state.entries.insert(path.clone(), None);
                Ok(())
            }
            BatchMutation::DeleteIfMatch { path, version } => {
                let mut state = self.lock_state()?;
                state.checks.push(BatchMutation::CheckObject {
                    path: path.clone(),
                    expected_version: Some(version.clone()),
                });
                state.entries.insert(path.clone(), None);
                Ok(())
            }
            BatchMutation::CheckObject {
                path,
                expected_version,
            } => {
                let mut state = self.lock_state()?;
                state.checks.push(BatchMutation::CheckObject {
                    path: path.clone(),
                    expected_version: expected_version.clone(),
                });
                Ok(())
            }
            BatchMutation::CheckPrefix {
                prefix,
                expected_entries,
            } => {
                let mut state = self.lock_state()?;
                state.checks.push(BatchMutation::CheckPrefix {
                    prefix: prefix.clone(),
                    expected_entries: expected_entries.clone(),
                });
                Ok(())
            }
        }
    }

    /// Number of buffered entries awaiting the final publication.
    pub fn pending_len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }

    /// 本轮缓冲里**实际待写**的对象事实（回执口径）。
    ///
    /// 发布成功后只能用这份事实结算 mutation：它是本轮真正写出去的路径与
    /// 内容摘要，而不是"计划要写什么"（复审 #3）。
    pub fn pending_publish_receipts(&self) -> Vec<PublishReceipt> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        state
            .entries
            .iter()
            .map(|(path, entry)| match entry {
                Some(BufferedValue::Content(content)) => PublishReceipt {
                    path: path.clone(),
                    op: "upsert".to_string(),
                    content_digest: Some(crate::sync::hash::content_md5(content)),
                },
                Some(BufferedValue::Version(version)) => PublishReceipt {
                    path: path.clone(),
                    op: "upsert".to_string(),
                    content_digest: Some(version.as_str().to_string()),
                },
                None => PublishReceipt {
                    path: path.clone(),
                    op: "delete".to_string(),
                    content_digest: None,
                },
            })
            .collect()
    }

    /// 按本地事务产出的对象决议应用本轮发布内容（复审 #7213234617 第 1 项）。
    ///
    /// - `Upload`：用该对象在 live 的最终字节替换缓冲内容（没有缓冲就补一条），
    ///   保证请求体来自已落盘决议而不是 transfer 阶段的历史缓冲；
    /// - `Consumed`/`Conflict`：从本轮批次移除，绝不能把本地旧版本或冲突版本推回。
    ///
    /// 返回替换/保留/缺失的条数。缺失表示决议要求上传但文件已不存在：这一条
    /// 直接不发布，调用方不得把旧内容顶上去。
    pub fn apply_publish_decisions(
        &self,
        decisions: &[crate::sync::decision::PublishDecision],
    ) -> crate::sync::decision::DecisionApplyOutcome {
        use crate::sync::decision::{DecisionApplyOutcome, PublishDisposition};
        let Ok(mut state) = self.state.lock() else {
            return DecisionApplyOutcome::default();
        };
        let mut outcome = DecisionApplyOutcome::default();
        for decision in decisions {
            match decision.disposition {
                PublishDisposition::Consumed | PublishDisposition::Conflict => {
                    if state.entries.remove(&decision.remote_path).is_some() {
                        outcome.withheld += 1;
                    }
                }
                PublishDisposition::Upload => {
                    let Some(live_path) = decision.live_path.as_ref() else {
                        continue;
                    };
                    match std::fs::read(live_path) {
                        Ok(bytes) => {
                            let same = state
                                .entries
                                .get(&decision.remote_path)
                                .and_then(|entry| entry.as_ref())
                                .is_some_and(|entry| {
                                    matches!(entry, BufferedValue::Content(current) if current == &bytes)
                                });
                            if same {
                                continue;
                            }
                            state.entries.insert(
                                decision.remote_path.clone(),
                                Some(BufferedValue::Content(bytes)),
                            );
                            outcome.rewritten += 1;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            // 决议之后对象又被删除：不发布旧内容，也不冒充删除。
                            state.entries.remove(&decision.remote_path);
                            outcome.missing += 1;
                        }
                        Err(error) => {
                            log::warn!(
                                "[sync] cannot read decided object for publish: {}: {error}",
                                live_path.display()
                            );
                            state.entries.remove(&decision.remote_path);
                            outcome.missing += 1;
                        }
                    }
                }
            }
        }
        outcome
    }

    /// 用该 target 已落盘的 manifest 字节替换本轮缓冲里的 manifest 写入。
    ///
    /// transfer 阶段预先生成的 manifest 描述的是当时的对象；本地提交之后
    /// 只有已落盘的那份才是真实决议。远端路径形如
    /// `<prefix>/app-meta/sync/manifest.sync.json`（App 根也可直接是该相对路径）。
    /// 落盘 manifest 不存在时直接移除缓冲条目，不发布过期描述。
    pub fn apply_manifest_decision(
        &self,
        prefix: &str,
        live_manifest: &std::path::Path,
    ) -> usize {
        let Ok(mut state) = self.state.lock() else {
            return 0;
        };
        let suffix = format!("/{}", crate::sync::lww::SYNC_MANIFEST_PATH);
        let matches = |path: &str| {
            (path == crate::sync::lww::SYNC_MANIFEST_PATH || path.ends_with(&suffix))
                && (prefix.is_empty() || path.starts_with(prefix))
        };
        let bytes = match std::fs::read(live_manifest) {
            Ok(bytes) => match serde_json::from_slice::<crate::sync::types::SyncManifest>(&bytes) {
                Ok(manifest) => manifest
                    .canonical_json()
                    .map(|text| text.into_bytes())
                    .unwrap_or(bytes),
                Err(_) => bytes,
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let keys: Vec<String> = state
                    .entries
                    .keys()
                    .filter(|path| matches(path))
                    .cloned()
                    .collect();
                for key in keys {
                    state.entries.remove(&key);
                }
                return 0;
            }
            Err(error) => {
                log::warn!(
                    "[sync] cannot read local manifest for publish: {}: {error}",
                    live_manifest.display()
                );
                return 0;
            }
        };
        let mut rewritten = 0usize;
        for (path, entry) in state.entries.iter_mut() {
            if !matches(path) {
                continue;
            }
            let same = entry
                .as_ref()
                .is_some_and(|value| matches!(value, BufferedValue::Content(current) if current == &bytes));
            if same {
                continue;
            }
            *entry = Some(BufferedValue::Content(bytes.clone()));
            rewritten += 1;
        }
        rewritten
    }

    /// Drop every buffered write and check, leaving the remote untouched.
    pub fn discard(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.entries.clear();
            state.checks.clear();
            state.baselines.clear();
            state.preconditioned_paths.clear();
            state.base_entries = None;
        }
    }

    /// Publish every buffered write as one atomic remote commit.
    ///
    /// A non-batch provider receives the same mutations sequentially; the
    /// wrapper exists for one-push semantics, not for provider-specific
    /// batching.
    pub fn flush(&self) -> Result<RoundFlushOutcome, ProviderError> {
        let (entries, checks, baselines, preconditioned_paths) = {
            let mut state = self.lock_state()?;
            (
                std::mem::take(&mut state.entries),
                std::mem::take(&mut state.checks),
                std::mem::take(&mut state.baselines),
                std::mem::take(&mut state.preconditioned_paths),
            )
        };
        if entries.is_empty() && checks.is_empty() {
            return Ok(RoundFlushOutcome {
                committed: false,
                result: BatchCommitResult {
                    revision: self
                        .base_revision
                        .clone()
                        .unwrap_or_else(|| RemoteVersion::new("")),
                    touched_paths: Vec::new(),
                },
            });
        }

        // 消除同内容/同版本/不存在的空 mutation：固定基线里已经成立的
        // ReuseVersion、基线里本来就没有的 Delete 都不应进入 batch（GitHub API
        // 会因此上传/删除多余对象，Git 也要多算一次 tree）。真正的内容变化
        // 仍然保留 Precondition 校验。
        let baseline: std::collections::HashMap<String, RemoteVersion> = self
            .all_remote_entries()?
            .into_iter()
            .map(|entry| (entry.path, entry.version))
            .collect();
        let mut mutations: Vec<BatchMutation> = entries
            .into_iter()
            .filter_map(|(path, entry)| match entry {
                Some(BufferedValue::Content(content)) => {
                    // 内容与基线逐字节相同（git blob OID 一致）时不进入 batch：
                    // 重复上传既浪费一次网络写，也会在 REST batch 上制造"内容没变
                    // 的提交"（Issue #1065 伪空提交）。
                    if let Some(existing) = baseline.get(&path) {
                        // 只对 git 对象名（40 位 hex）比较；非 git 后端的版本串
                        // 不参与，避免误判。
                        let existing = existing.as_str();
                        if crate::sync::hash::is_legacy_git_blob_oid(existing)
                            && crate::sync::hash::git_blob_oid(&content)
                                .map(|digest| digest == existing)
                                .unwrap_or(false)
                        {
                            return None;
                        }
                    }
                    Some(BatchMutation::Put { path, content })
                }
                Some(BufferedValue::Version(version)) => {
                    if baseline.get(&path) == Some(&version) {
                        None
                    } else {
                        Some(BatchMutation::ReuseVersion { path, version })
                    }
                }
                None => {
                    if baseline.contains_key(&path) {
                        Some(BatchMutation::Delete { path })
                    } else {
                        None
                    }
                }
            })
            .collect();
        let mut checked_objects: BTreeSet<String> = BTreeSet::new();
        for check in &checks {
            if let Some(sanitized) = self.sanitize_check(check, &baselines)? {
                if let BatchMutation::CheckObject { path, .. } = &sanitized {
                    checked_objects.insert(path.clone());
                }
                mutations.push(sanitized);
            }
        }
        // A write/delete precondition validated against the overlay must still
        // hold on the real remote tree the final commit is built from.
        for path in preconditioned_paths {
            if checked_objects.contains(&path) {
                continue;
            }
            let expected_version = self.baseline_from(&baselines, &path)?;
            mutations.push(BatchMutation::CheckObject {
                path,
                expected_version,
            });
        }

        // 过滤后可能只剩"什么也不用写"：不再发起一次空提交。
        if mutations.is_empty() {
            return Ok(RoundFlushOutcome {
                committed: false,
                result: BatchCommitResult {
                    revision: self
                        .base_revision
                        .clone()
                        .unwrap_or_else(|| RemoteVersion::new("")),
                    touched_paths: Vec::new(),
                },
            });
        }

        if self.inner.capabilities().batch {
            let result = self.inner.commit_batch_at(
                self.base_revision.as_ref(),
                &mutations,
                &format!("sujian: sync round ({} mutations)", mutations.len()),
            )?;
            let committed = Some(&result.revision) != self.base_revision.as_ref();
            return Ok(RoundFlushOutcome { committed, result });
        }

        // Non-batch fallback: run baseline checks first, then apply mutations
        // one by one.  This path is not used by the built-in providers.
        for mutation in &mutations {
            match mutation {
                BatchMutation::CheckObject {
                    path,
                    expected_version,
                } => {
                    let actual = self.inner.read(path)?.map(|object| object.version);
                    if &actual != expected_version {
                        return Err(ProviderError::PreconditionFailed {
                            path: path.clone(),
                            reason: "remote object changed before final publish".to_string(),
                        });
                    }
                }
                BatchMutation::CheckPrefix {
                    prefix,
                    expected_entries,
                } => {
                    let actual = self.inner.list(prefix)?;
                    if &actual != expected_entries {
                        return Err(ProviderError::PreconditionFailed {
                            path: prefix.clone(),
                            reason: "remote prefix changed before final publish".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }

        let mut touched_paths = Vec::new();
        for mutation in &mutations {
            match mutation {
                BatchMutation::Put { path, content } => {
                    self.inner
                        .write(path, content, WritePrecondition::Unconditional)?;
                    touched_paths.push(path.clone());
                }
                BatchMutation::ReuseVersion { path, .. } => {
                    // A non-batch provider cannot reuse a remote blob directly;
                    // the caller must have supplied new content instead.
                    return Err(ProviderError::Other {
                        reason: format!("non-batch provider cannot reuse version for path {path}"),
                    });
                }
                BatchMutation::Delete { path } => {
                    self.inner.delete(path, DeletePrecondition::Unconditional)?;
                    touched_paths.push(path.clone());
                }
                BatchMutation::DeleteIfMatch { .. }
                | BatchMutation::CheckObject { .. }
                | BatchMutation::CheckPrefix { .. } => {}
            }
        }
        let committed = !touched_paths.is_empty();
        Ok(RoundFlushOutcome {
            committed,
            result: BatchCommitResult {
                revision: RemoteVersion::new("round"),
                touched_paths,
            },
        })
    }
}

impl SyncProvider for RoundProvider<'_> {
    fn set_operation_id(&self, operation_id: Option<&str>) {
        self.inner.set_operation_id(operation_id);
    }

    fn network_metrics(&self) -> ProviderNetworkMetrics {
        self.inner.network_metrics()
    }

    fn capabilities(&self) -> SyncCapabilities {
        self.inner.capabilities()
    }

    fn remote_revision(&self) -> Result<Option<String>, ProviderError> {
        self.inner.remote_revision()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        self.overlay_entries(prefix)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        let buffered = {
            let state = self.lock_state()?;
            state.entries.get(path).cloned()
        };
        match buffered {
            Some(Some(BufferedValue::Content(content))) => {
                let version = Self::buffered_version(&content);
                Ok(Some(RemoteObject {
                    path: path.to_string(),
                    content,
                    version,
                }))
            }
            Some(Some(BufferedValue::Version(version))) => {
                // The target path is new (for example a stable path replacing
                // an old generation path).  Resolve the reused blob by finding
                // a currently reachable path with the same version; delegating
                // `read(new_path)` would incorrectly return None.
                let entries = self.all_remote_entries()?;
                let source = entries.into_iter().find(|entry| entry.version == version);
                let Some(source) = source else {
                    return Err(ProviderError::PreconditionFailed {
                        path: path.to_string(),
                        reason: format!(
                            "reused blob version {} is not reachable",
                            version.as_str()
                        ),
                    });
                };
                let object = self.inner.read(&source.path)?;
                Ok(object.map(|mut object| {
                    object.path = path.to_string();
                    object.version = version;
                    object
                }))
            }
            Some(None) => Ok(None),
            None => self.inner.read(path),
        }
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        self.check_write_precondition(path, &precondition)?;
        let version = Self::buffered_version(content);
        let mut state = self.lock_state()?;
        state.entries.insert(
            path.to_string(),
            Some(BufferedValue::Content(content.to_vec())),
        );
        Ok(version)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        self.check_delete_precondition(path, &precondition)?;
        let mut state = self.lock_state()?;
        state.entries.insert(path.to_string(), None);
        Ok(())
    }

    fn commit_batch(
        &self,
        mutations: &[BatchMutation],
        _message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        for mutation in mutations {
            self.apply_mutation(mutation)?;
        }
        Ok(BatchCommitResult {
            revision: RemoteVersion::new("round"),
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

    fn diagnose(&self) -> Result<crate::sync::types::SyncDiagnosticsResult, ProviderError> {
        self.inner.diagnose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::provider::memory::MemoryProvider;
    use crate::sync::provider::SyncProvider;

    #[test]
    fn writes_are_deferred_until_flush_and_visible_to_reads() {
        let inner = MemoryProvider::new();
        let round = RoundProvider::new(&inner, None);

        round
            .write("a.txt", b"a", WritePrecondition::CreateNew)
            .unwrap();
        assert!(inner.read("a.txt").unwrap().is_none());
        assert_eq!(round.read("a.txt").unwrap().unwrap().content, b"a".to_vec());

        round.flush().unwrap();
        assert_eq!(inner.read("a.txt").unwrap().unwrap().content, b"a".to_vec());
    }

    #[test]
    fn write_preconditions_run_against_the_overlay() {
        let inner = MemoryProvider::new();
        let existing = inner
            .write("a.txt", b"remote", WritePrecondition::Unconditional)
            .unwrap();
        let round = RoundProvider::new(&inner, None);

        assert!(matches!(
            round.write("a.txt", b"x", WritePrecondition::CreateNew),
            Err(ProviderError::PreconditionFailed { .. })
        ));
        assert!(matches!(
            round.write(
                "a.txt",
                b"x",
                WritePrecondition::IfMatch(RemoteVersion::new("stale"))
            ),
            Err(ProviderError::PreconditionFailed { .. })
        ));

        round
            .write("a.txt", b"next", WritePrecondition::IfMatch(existing))
            .unwrap();
        round.flush().unwrap();
        assert_eq!(inner.read("a.txt").unwrap().unwrap().content, b"next");
    }

    #[test]
    fn virtual_round_versions_stay_inside_the_overlay() {
        let inner = MemoryProvider::new();
        let round = RoundProvider::new(&inner, None);

        let first = round
            .write("catalog.json", b"one", WritePrecondition::CreateNew)
            .unwrap();
        assert!(first.as_str().starts_with("round:"));
        let second = round
            .write(
                "catalog.json",
                b"two",
                WritePrecondition::IfMatch(first.clone()),
            )
            .unwrap();
        assert!(matches!(
            round.write("catalog.json", b"three", WritePrecondition::IfMatch(first)),
            Err(ProviderError::PreconditionFailed { .. })
        ));

        round.flush().unwrap();
        assert_eq!(inner.read("catalog.json").unwrap().unwrap().content, b"two");
        assert!(second.as_str().starts_with("round:"));
    }

    #[test]
    fn flush_rechecks_preconditions_against_the_real_remote() {
        let inner = MemoryProvider::new();
        let baseline = inner
            .write("catalog.json", b"one", WritePrecondition::Unconditional)
            .unwrap();
        let round = RoundProvider::new(&inner, None);
        round
            .write("catalog.json", b"two", WritePrecondition::IfMatch(baseline))
            .unwrap();

        // Another client advances the branch after this round was planned.
        inner
            .write("catalog.json", b"other", WritePrecondition::Unconditional)
            .unwrap();

        assert!(matches!(
            round.flush(),
            Err(ProviderError::PreconditionFailed { .. })
        ));
        assert_eq!(
            inner.read("catalog.json").unwrap().unwrap().content,
            b"other"
        );
    }

    #[test]
    fn explicit_checks_are_sanitized_to_real_baselines() {
        let inner = MemoryProvider::new();
        let round = RoundProvider::new(&inner, None);
        let virtual_version = round
            .write("catalog.json", b"one", WritePrecondition::CreateNew)
            .unwrap();

        round
            .commit_batch(
                &[BatchMutation::CheckObject {
                    path: "catalog.json".to_string(),
                    expected_version: Some(virtual_version),
                }],
                "test",
            )
            .unwrap();

        round.flush().unwrap();
        assert_eq!(inner.read("catalog.json").unwrap().unwrap().content, b"one");
    }
}
