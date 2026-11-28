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
    Content(std::sync::Arc<[u8]>),
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
    /// 本轮真正进入远端提交的对象事实（评审 #7214314073 第 2 项）。
    ///
    /// 由 `flush` 在过滤空 mutation、校验前置条件之后生成，而不是提交前按
    /// 缓冲推测；调用方结算 local mutation 时只认这份回执。
    pub receipts: Vec<PublishReceipt>,
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
    /// provider 用提交后 tree 证明的实际远端版本（Git blob OID）；拿不到时为空。
    pub remote_version: Option<String>,
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
        // 固定 HEAD 的基线只枚举请求的 prefix（Git 走该子树，REST 也限定范围），
        // 不再先拉全库再本地过滤。
        let mut entries: BTreeMap<String, RemoteVersion> = self
            .inner
            .list(prefix)?
            .into_iter()
            .map(|entry| (entry.path, entry.version))
            .collect();
        let needle = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
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
                    .insert(
                        path.clone(),
                        Some(BufferedValue::Content(std::sync::Arc::from(content.as_slice()))),
                    );
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

    /// 把一个已经完整生成并验证过的局部批次并入全局缓冲。
    ///
    /// 每个 target 的对象与索引先在自己的 RoundProvider 里生成；只有全部
    /// 写入成功后才调用这里，失败 target 的局部缓冲直接丢弃，绝不污染
    /// 其它 target 的发布批次。
    pub fn absorb(&self, other: &RoundProvider<'_>) {
        let Ok(mut other_state) = other.state.lock() else {
            return;
        };
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        for (path, entry) in std::mem::take(&mut other_state.entries) {
            state.entries.insert(path, entry);
        }
        state.checks.append(&mut other_state.checks);
        for (path, baseline) in std::mem::take(&mut other_state.baselines) {
            state.baselines.entry(path).or_insert(baseline);
        }
        state
            .preconditioned_paths
            .append(&mut other_state.preconditioned_paths);
    }

    /// Number of buffered entries awaiting the final publication.
    pub fn pending_len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }

    /// 按本地提交产出的不可变决议重建本轮发布批次（复审 #7214105551 第 2 项）。
    ///
    /// `decisions` 里的内容在提交阶段已冻结：这里只做"用决议内容替换缓冲"，
    /// 不再回读 live，也不猜测。纯远端消费的决议从批次移除。
    pub fn apply_durable_decisions(
        &self,
        decisions: &[(String, crate::sync::decision::DurableObjectDecision)],
    ) -> crate::sync::decision::DecisionApplyOutcome {
        use crate::sync::decision::{DecisionApplyOutcome, DecisionOp};
        let Ok(mut state) = self.state.lock() else {
            return DecisionApplyOutcome::default();
        };
        let mut outcome = DecisionApplyOutcome::default();
        for (remote_path, decision) in decisions {
            if !decision.is_publishable() {
                // 纯远端消费/未决冲突：都不允许把 transfer 预生成的本地旧内容
                // 再推回去，必须沿用固定远端基线。
                if state.entries.remove(remote_path).is_some() {
                    outcome.withheld += 1;
                }
                continue;
            }
            match decision.op {
                DecisionOp::Delete => {
                    // 显式删除必须覆盖 transfer 阶段可能生成的旧 Put 条目，
                    // 否则旧内容仍会随本轮发布（评审 #7214314073 第 2 项）。
                    let had_content = state
                        .entries
                        .get(remote_path)
                        .is_some_and(|entry| entry.is_some());
                    if had_content {
                        outcome.rewritten += 1;
                    }
                    state.entries.insert(remote_path.clone(), None);
                }
                DecisionOp::Upsert => {
                    let Some(content) = decision.content.as_ref() else {
                        outcome.missing += 1;
                        state.entries.remove(remote_path);
                        continue;
                    };
                    let same = state
                        .entries
                        .get(remote_path)
                        .and_then(|entry| entry.as_ref())
                        .is_some_and(|entry| {
                            matches!(entry, BufferedValue::Content(current) if current[..] == content[..])
                        });
                    if same {
                        continue;
                    }
                    state.entries.insert(
                        remote_path.clone(),
                        Some(BufferedValue::Content(content.clone())),
                    );
                    outcome.rewritten += 1;
                }
            }
        }
        outcome
    }

    /// 只保留有 durable 决议或属于生命周期/派生元数据的对象。
    ///
    /// transfer 阶段预生成的普通对象条目不能自行进入发布：没有决议的对象
    /// 保持远端原值（评审 #7214314073 第 2 项）。生命周期的 catalog/
    /// generation/GC 元数据不是业务对象，由各自流程直接写入。
    pub fn retain_decided_objects(
        &self,
        decisions: &[(String, crate::sync::decision::DurableObjectDecision)],
    ) -> usize {
        let covered: std::collections::BTreeSet<&str> = decisions
            .iter()
            .map(|(remote_path, _)| remote_path.as_str())
            .collect();
        let Ok(mut state) = self.state.lock() else {
            return 0;
        };
        let before = state.entries.len();
        state.entries.retain(|path, value| {
            covered.contains(path.as_str()) || is_lifecycle_publish_path(path, value.is_none())
        });
        before.saturating_sub(state.entries.len())
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
                    path_versions: std::collections::BTreeMap::new(),
                },
                receipts: Vec::new(),
            });
        }

        // 消除同内容/同版本/不存在的空 mutation：固定基线里已经成立的
        // ReuseVersion、基线里本来就没有的 Delete 都不应进入 batch（GitHub API
        // 会因此上传/删除多余对象，Git 也要多算一次 tree）。真正的内容变化
        // 仍然保留 Precondition 校验。
        // 只为本批实际涉及的路径读取固定基线版本；空 Put/Delete 的消除
        // 不再依赖全库枚举（Git 一次按路径查 OID）。
        let baseline_paths: std::collections::BTreeSet<String> = entries.keys().cloned().collect();
        let mut baseline: std::collections::HashMap<String, RemoteVersion> =
            std::collections::HashMap::with_capacity(baseline_paths.len());
        for path in baseline_paths {
            if let Some(version) = self.real_baseline_for(&path)? {
                baseline.insert(path, version);
            }
        }
        let mut mutations: Vec<BatchMutation> = Vec::new();
        // 空 mutation 不代表"没有确认"：与固定 HEAD 已一致的对象直接把
        // 该路径的版本事实作为回执返回，调用方据此结算，不生成空提交。
        let mut satisfied: Vec<PublishReceipt> = Vec::new();
        for (path, entry) in entries {
            match entry {
                Some(BufferedValue::Content(content)) => {
                    // 内容与基线逐字节相同（git blob OID 一致）时不进入 batch：
                    // 重复上传既浪费一次网络写，也会在 REST batch 上制造"内容没变
                    // 的提交"（Issue #1065 伪空提交）。
                    let existing = baseline
                        .get(&path)
                        .map(|version| version.as_str())
                        .filter(|version| crate::sync::hash::is_legacy_git_blob_oid(version));
                    if existing.is_some_and(|existing| {
                        crate::sync::hash::git_blob_oid(&content)
                            .map(|digest| digest == existing)
                            .unwrap_or(false)
                    }) {
                        satisfied.push(PublishReceipt {
                            path,
                            op: "upsert".to_string(),
                            content_digest: Some(crate::sync::hash::content_md5(&content)),
                            remote_version: existing.map(str::to_string),
                        });
                    } else {
                        mutations.push(BatchMutation::Put {
                            path,
                            content: content.to_vec(),
                        });
                    }
                }
                Some(BufferedValue::Version(version)) => {
                    if baseline.get(&path) != Some(&version) {
                        mutations.push(BatchMutation::ReuseVersion { path, version });
                    }
                }
                None => {
                    if baseline.contains_key(&path) {
                        mutations.push(BatchMutation::Delete { path });
                    } else {
                        // 固定 HEAD 里本来就不存在：删除已满足。
                        satisfied.push(PublishReceipt {
                            path,
                            op: "delete".to_string(),
                            content_digest: None,
                            remote_version: None,
                        });
                    }
                }
            }
        }
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

        // 只把真正进入提交的 Put/ReuseVersion/Delete 变成回执；CheckObject/
        // CheckPrefix 不是对象事实。空内容过滤后没有事实就不确认。
        let mut receipts: Vec<PublishReceipt> = mutations
            .iter()
            .filter_map(|mutation| match mutation {
                BatchMutation::Put { path, content } => Some(PublishReceipt {
                    path: path.clone(),
                    op: "upsert".to_string(),
                    content_digest: Some(crate::sync::hash::content_md5(content)),
                    remote_version: None,
                }),
                BatchMutation::ReuseVersion { path, version } => Some(PublishReceipt {
                    path: path.clone(),
                    // 复用远端版本不是新的内容上传：回执里不能把它伪装成
                    // upsert+内容摘要（MD5 与 Git OID 不可比），调用方应按
                    // 提交后的 known_files 一致性确认。
                    op: "reuse".to_string(),
                    content_digest: Some(version.as_str().to_string()),
                    remote_version: None,
                }),
                BatchMutation::Delete { path } => Some(PublishReceipt {
                    path: path.clone(),
                    op: "delete".to_string(),
                    content_digest: None,
                    remote_version: None,
                }),
                _ => None,
            })
            .collect();
        receipts.extend(satisfied);

        // 过滤后可能只剩"什么也不用写"：不再发起一次空提交；已满足的对象
        // 仍然带回执供调用方结算。
        if mutations.is_empty() {
            return Ok(RoundFlushOutcome {
                committed: false,
                result: BatchCommitResult {
                    revision: self
                        .base_revision
                        .clone()
                        .unwrap_or_else(|| RemoteVersion::new("")),
                    touched_paths: Vec::new(),
                    path_versions: std::collections::BTreeMap::new(),
                },
                receipts,
            });
        }

        if self.inner.capabilities().batch {
            let result = self.inner.commit_batch_at(
                self.base_revision.as_ref(),
                &mutations,
                &format!("sujian: sync round ({} mutations)", mutations.len()),
            )?;
            let committed = Some(&result.revision) != self.base_revision.as_ref();
            for receipt in &mut receipts {
                if let Some(version) = result.path_versions.get(&receipt.path) {
                    receipt.remote_version = Some(version.as_str().to_string());
                }
            }
            return Ok(RoundFlushOutcome {
                committed,
                result,
                receipts,
            });
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
        let receipts = receipts
            .into_iter()
            .filter(|receipt| touched_paths.iter().any(|path| path == &receipt.path))
            .collect();
        Ok(RoundFlushOutcome {
            committed,
            result: BatchCommitResult {
                revision: RemoteVersion::new("round"),
                touched_paths,
                path_versions: std::collections::BTreeMap::new(),
            },
            receipts,
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
                    content: content.to_vec(),
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
            Some(BufferedValue::Content(std::sync::Arc::from(content))),
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
            path_versions: std::collections::BTreeMap::new(),
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
    fn flush_and_prefixed_listing_do_not_enumerate_the_whole_remote() {
        let inner = MemoryProvider::new();
        for path in ["projects/P/a.txt", "projects/P/b.txt", "other/c.txt"] {
            inner
                .write(path, b"x", WritePrecondition::Unconditional)
                .unwrap();
        }
        let round = RoundProvider::new(&inner, None);
        round
            .write("projects/P/d.txt", b"new", WritePrecondition::CreateNew)
            .unwrap();
        round.flush().unwrap();
        assert!(
            round
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .base_entries
                .is_none(),
            "flush 不得为了消除空变更而缓存全库条目"
        );
        let listed = round.list("projects/P").unwrap();
        assert_eq!(listed.len(), 3);
        assert!(
            round
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .base_entries
                .is_none(),
            "按 prefix 列目录不得退化成全库枚举缓存"
        );
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
    fn retain_keeps_manifest_storage_in_same_commit() {
        use crate::sync::decision::{DecisionOp, DecisionOrigin, DurableObjectDecision};

        let inner = MemoryProvider::new();
        let round = RoundProvider::new(&inner, None);
        for (path, bytes) in [
            (
                "projects/P/app-meta/sync/manifest/root.json",
                b"root".as_slice(),
            ),
            (
                "projects/P/app-meta/sync/manifest/objects/abc.json",
                b"obj".as_slice(),
            ),
            (
                "projects/P/volumes/v1/chapters/c1/chapter.md",
                b"one".as_slice(),
            ),
            (
                "projects/P/volumes/v1/chapters/c2/chapter.md",
                b"two".as_slice(),
            ),
        ] {
            round
                .write(path, bytes, WritePrecondition::CreateNew)
                .unwrap();
        }
        let decision = DurableObjectDecision {
            transaction_id: "tx".to_string(),
            identity: "volumes/v1/chapters/c1/chapter.md".to_string(),
            op: DecisionOp::Upsert,
            base_hash: None,
            final_hash: "hash".to_string(),
            final_version: Some(1),
            mutation_seq: 1,
            origin: DecisionOrigin::Local,
            content: Some(std::sync::Arc::from(b"one".to_vec())),
            dependencies: Vec::new(),
        };
        // 旧单文件不再能成为发布依据；但迁移清理的 Delete 必须保留。
        round
            .write(
                "projects/P/app-meta/sync/manifest.sync.json",
                b"legacy".as_slice(),
                WritePrecondition::CreateNew,
            )
            .unwrap();
        round
            .delete(
                "projects/P/app-meta/sync/manifest-shards/volumes.json",
                DeletePrecondition::Unconditional,
            )
            .unwrap();
        let removed = round.retain_decided_objects(&[(
            "projects/P/volumes/v1/chapters/c1/chapter.md".to_string(),
            decision,
        )]);
        assert_eq!(removed, 2, "未决议业务对象与旧单文件 Put 必须被过滤");
        round.flush().unwrap();

        assert!(inner
            .read("projects/P/app-meta/sync/manifest/root.json")
            .unwrap()
            .is_some());
        assert!(inner
            .read("projects/P/app-meta/sync/manifest/objects/abc.json")
            .unwrap()
            .is_some());
        assert!(inner
            .read("projects/P/volumes/v1/chapters/c1/chapter.md")
            .unwrap()
            .is_some());
        assert!(inner
            .read("projects/P/volumes/v1/chapters/c2/chapter.md")
            .unwrap()
            .is_none());
        assert!(
            inner
                .read("projects/P/app-meta/sync/manifest.sync.json")
                .unwrap()
                .is_none(),
            "旧单文件 Put 不得发布"
        );
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

/// 生命周期/派生元数据路径：不是业务对象的发布决议，允许由各自流程直写。
///
/// 同步清单（v2 根/索引/对象，以及迁移期的旧单文件与 namespace 分片）
/// 与业务对象属于同一次发布提交：根切换必须和本轮对象决议一起落远端，
/// 因此这些路径不能被决议过滤掉（评论 #7219676521 第一项）。
fn is_lifecycle_publish_path(path: &str, is_delete: bool) -> bool {
    path.ends_with("/app-meta/sync/targets.sync.json")
        || is_manifest_publish_path(path, is_delete)
        || path.contains("/__generations__/")
        || path.starts_with("__generations__/")
        || path.contains("/__gc__/")
        || path.starts_with("__gc__/")
}

/// 远端清单存储路径。
///
/// v2 `manifest/` 的根/索引/对象是同一 durable 提交的派生路径，允许随业务
/// 对象一起发布。旧单文件与旧 `manifest-shards/` 只在迁移清理时**删除**，
/// 不再允许把旧单文件本身当作新的发布依据（评论 #7219676521 第一项）。
fn is_manifest_publish_path(path: &str, is_delete: bool) -> bool {
    let v2 =
        path.contains("/app-meta/sync/manifest/") || path.starts_with("app-meta/sync/manifest/");
    if v2 {
        return true;
    }
    let legacy = path == crate::sync::lww::SYNC_MANIFEST_PATH
        || path.ends_with("/app-meta/sync/manifest.sync.json")
        || path.contains("/app-meta/sync/manifest-shards/")
        || path.starts_with("app-meta/sync/manifest-shards/");
    legacy && is_delete
}
