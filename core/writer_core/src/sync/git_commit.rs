//! #644 评论 5475805198 第2节：Git metadata finalize 的原子提交边界。
//!
//! 从 `git_staging.rs` 拆出 finalize 逻辑，本模块负责：
//! - `GitMetadataSnapshot`：finalize 前的仓库状态快照，供崩溃恢复使用。
//! - `prepare_git_finalize()`：捕获快照。
//! - `commit_git_finalize()`：应用 Git metadata 变更，失败时自动 rollback。
//! - `rollback_git_finalize()`：从快照恢复。
//! - `recover_git_finalize()`：崩溃恢复（`FilesCommittedPendingGit` 状态）。
//!
//! `git_staging.rs` 只保留 seed / `GitSeedState`。
//!
//! ## HEAD 并发保护
//!
//! #644 评论 5475805198 第3节：
//! - `finalize_unborn()` 使用 `find_reference("HEAD")` 读取未 resolve 的 HEAD，
//!   确认 `symbolic_target() == seed head_ref` 且目标 branch ref 仍不存在。
//! - `finalize_existing()` 除 `head_ref` CAS 外，还确认 HEAD 仍指向同一 branch。
//! - `finalize_detached()` 使用 `reference_matching("HEAD", ...)` 做真正的 CAS。
//!
//! ## 临时目录 RAII
//!
//! #644 评论 5475805198 第4节：
//! `finalize_not_git_repo()` 的 `.git.sujian-tmp-<uuid>` 用 `TmpDirGuard` 保证
//! 任何返回路径都删除。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::sync::git_staging::GitSeedState;

// ── 快照类型 ──

/// #644 评论 5475805198 第2节 + #644 评论 5476546134 第4节：
/// 单个引用的快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RefSnapshot {
    /// 引用不存在（finalize 会创建它）。
    DidNotExist,
    /// 引用存在且是 direct OID。
    Existed { oid: String },
    /// 引用存在且是 symbolic（如 HEAD → refs/heads/main）。
    Symbolic { target: String },
}

/// #644 评论 5476546134 第4节：index 快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexSnapshot {
    /// index 文件不存在。
    Missing,
    /// index 文件的原始字节。
    Bytes(Vec<u8>),
}

/// #644 评论 5475805198 第2节 + #644 评论 5476546134 第4节：
/// finalize 前的 Git metadata 快照。
///
/// 供崩溃恢复使用：进程在 `SaveTransaction.commit()` 和 `commit_git_finalize()`
/// 之间退出时，下次启动可通过本快照完成或回滚 Git metadata。
///
/// #644 评论 5476546134 第4节：不再混用 `head_snapshot + head_ref`。
/// `head` 只对应 HEAD 引用本身；`refs` 记录所有本轮会修改的 branch/remote refs。
/// rollback 按 ref 名逐项恢复/删除，所有错误直接 `?` 传播。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitMetadataSnapshot {
    /// HEAD 引用的快照（symbolic 或 detached）。
    pub head: RefSnapshot,
    /// 所有本轮会修改的 branch/remote refs 的快照。
    /// key = ref 名（如 `refs/heads/main`、`refs/remotes/origin/main`）。
    /// value = finalize 前的状态（`DidNotExist` 表示 finalize 会新建）。
    pub refs: std::collections::BTreeMap<String, RefSnapshot>,
    /// index 快照。
    pub index: IndexSnapshot,
    /// finalize 前 live 是否已是 Git repo。
    pub repo_existed: bool,
}

/// #644 评论 5476546134 第2节：Git finalize 崩溃恢复记录。
///
/// 写入 `TransactionManifest.git_finalize`，使重启后能独立从 live + transaction 目录
/// 完成恢复，不依赖可能残缺的 staging run。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFinalizeRecoveryRecord {
    /// seed 时记录的 live Git 仓库状态（序列化友好版本）。
    pub seed_state: SerializableGitSeedState,
    /// finalize 前的 Git metadata 快照。
    pub metadata_snapshot: GitMetadataSnapshot,
}

/// #644 评论 5476546134 第2节：`GitSeedState` 的序列化友好版本。
///
/// `git2::Oid` 不实现 `Serialize/Deserialize`，用 hex 字符串存储。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SerializableGitSeedState {
    NotGitRepo,
    Unborn { head_ref: String },
    Existing { head_ref: String, head_oid: String },
    Detached { head_oid: String },
}

impl SerializableGitSeedState {
    /// 从 `GitSeedState` 转换。
    pub fn from_seed_state(state: &crate::sync::git_staging::GitSeedState) -> Self {
        use crate::sync::git_staging::GitSeedState;
        match state {
            GitSeedState::NotGitRepo => Self::NotGitRepo,
            GitSeedState::Unborn { head_ref } => Self::Unborn {
                head_ref: head_ref.clone(),
            },
            GitSeedState::Existing { head_ref, head_oid } => Self::Existing {
                head_ref: head_ref.clone(),
                head_oid: head_oid.to_string(),
            },
            GitSeedState::Detached { head_oid } => Self::Detached {
                head_oid: head_oid.to_string(),
            },
        }
    }

    /// 还原为 `GitSeedState`。
    pub fn to_seed_state(
        &self,
    ) -> std::result::Result<crate::sync::git_staging::GitSeedState, String> {
        use crate::sync::git_staging::GitSeedState;
        match self {
            Self::NotGitRepo => Ok(GitSeedState::NotGitRepo),
            Self::Unborn { head_ref } => Ok(GitSeedState::Unborn {
                head_ref: head_ref.clone(),
            }),
            Self::Existing { head_ref, head_oid } => {
                let oid =
                    git2::Oid::from_str(head_oid).map_err(|e| format!("invalid head_oid: {e}"))?;
                Ok(GitSeedState::Existing {
                    head_ref: head_ref.clone(),
                    head_oid: oid,
                })
            }
            Self::Detached { head_oid } => {
                let oid =
                    git2::Oid::from_str(head_oid).map_err(|e| format!("invalid head_oid: {e}"))?;
                Ok(GitSeedState::Detached { head_oid: oid })
            }
        }
    }
}

// ── RAII 临时目录守卫 ──

/// #644 评论 5475805198 第4节：RAII 守卫，保证临时目录在 drop 时删除。
struct TmpDirGuard(Option<PathBuf>);

#[allow(clippy::expect_used)]
impl TmpDirGuard {
    fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    fn path(&self) -> &Path {
        // SAFETY: 构造后 `0` 始终为 Some，直到 `disarm()` 置 None。
        self.0.as_ref().expect("TmpDirGuard already disarmed")
    }

    /// 取消自动删除（成功 rename 后调用），返回路径。
    fn disarm(&mut self) -> PathBuf {
        self.0.take().expect("TmpDirGuard already disarmed")
    }
}

impl Drop for TmpDirGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

// ── 公共 API ──

/// #644 评论 5475805198 第2节 + #644 评论 5476546134 第4节：
/// 捕获 finalize 前的 Git metadata 快照。
///
/// 在 `SaveTransaction.commit()` 之前调用，快照写入 manifest 供崩溃恢复。
///
/// #644 评论 5476546134 第4节：重写快照模型——
/// - `head`：只对应 HEAD 引用本身（symbolic / detached）。
/// - `refs`：所有本轮会修改的 branch/remote refs，包括 staging 将要写入的
///   `refs/remotes/*`（live 不存在的显式记录 `DidNotExist`）。
/// - `index`：`IndexSnapshot::Missing` 或 `IndexSnapshot::Bytes`。
/// - `repo_existed`：finalize 前 live 是否已是 Git repo。
#[allow(clippy::excessive_nesting)]
pub fn prepare_git_finalize(
    live_root: &Path,
    seed_state: &GitSeedState,
    staging_root: &Path,
) -> Result<GitMetadataSnapshot> {
    // NotGitRepo 时返回最小快照，不尝试打开 repo。
    if matches!(seed_state, GitSeedState::NotGitRepo) {
        return Ok(GitMetadataSnapshot {
            head: RefSnapshot::DidNotExist,
            refs: std::collections::BTreeMap::new(),
            index: IndexSnapshot::Missing,
            repo_existed: false,
        });
    }

    crate::storage::git_runtime::ensure_initialized()?;

    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "prepare_git_finalize: open live repo: {e}"
        )))
    })?;

    // 捕获 HEAD 快照。
    let head = match live_repo.find_reference("HEAD") {
        Ok(head_ref) => {
            if let Some(sym_target) = head_ref.symbolic_target() {
                RefSnapshot::Symbolic {
                    target: sym_target.to_string(),
                }
            } else if let Some(oid) = head_ref.target() {
                RefSnapshot::Existed {
                    oid: oid.to_string(),
                }
            } else {
                RefSnapshot::DidNotExist
            }
        }
        Err(_) => RefSnapshot::DidNotExist,
    };

    // 捕获 index 快照。
    let index_path = live_root.join(".git").join("index");
    let index = if index_path.exists() {
        // #644 评论 5476546134 第4节：读取失败不能伪造"空 index"，
        // 直接 ? 返回错误。snapshot 本身就是回滚事实，不能降级。
        IndexSnapshot::Bytes(fs::read(&index_path)?)
    } else {
        IndexSnapshot::Missing
    };

    // 收集所有本轮会修改的 refs。
    let mut refs = std::collections::BTreeMap::new();

    // 1. head_ref（branch ref 或 detached HEAD）。
    let head_ref = match seed_state {
        GitSeedState::Unborn { head_ref } | GitSeedState::Existing { head_ref, .. } => {
            head_ref.clone()
        }
        GitSeedState::Detached { .. } => "HEAD".to_string(),
        GitSeedState::NotGitRepo => String::new(),
    };
    if !head_ref.is_empty() && head_ref != "HEAD" {
        let snap = snapshot_ref(&live_repo, &head_ref);
        refs.insert(head_ref, snap);
    }

    // 2. live 已有的 remote refs。
    if let Ok(live_refs) = live_repo.references() {
        for reference in live_refs.flatten() {
            let Some(name) = reference.name() else {
                continue;
            };
            if !name.starts_with("refs/remotes/") {
                continue;
            }
            refs.insert(name.to_string(), snapshot_ref_from_repo_ref(&reference));
        }
    }

    // 3. staging 将要写入的 remote refs（live 不存在的显式记录 DidNotExist）。
    if let Ok(staging_repo) = git2::Repository::open(staging_root) {
        if let Ok(staging_refs) = staging_repo.references() {
            for reference in staging_refs.flatten() {
                let Some(name) = reference.name() else {
                    continue;
                };
                if !name.starts_with("refs/remotes/") {
                    continue;
                }
                refs.entry(name.to_string())
                    .or_insert(RefSnapshot::DidNotExist);
            }
        }
    }

    Ok(GitMetadataSnapshot {
        head,
        refs,
        index,
        repo_existed: true,
    })
}

/// 从 git2 Reference 构造 RefSnapshot。
fn snapshot_ref_from_repo_ref(reference: &git2::Reference<'_>) -> RefSnapshot {
    if let Some(oid) = reference.target() {
        RefSnapshot::Existed {
            oid: oid.to_string(),
        }
    } else if let Some(sym) = reference.symbolic_target() {
        RefSnapshot::Symbolic {
            target: sym.to_string(),
        }
    } else {
        RefSnapshot::DidNotExist
    }
}

/// 按 ref 名在 repo 中查找并构造快照。
fn snapshot_ref(repo: &git2::Repository, ref_name: &str) -> RefSnapshot {
    match repo.find_reference(ref_name) {
        Ok(reference) => snapshot_ref_from_repo_ref(&reference),
        Err(_) => RefSnapshot::DidNotExist,
    }
}

// ── Git finalize 错误类型 ──

/// #644 评论 5476546134 第4节：Git finalize 错误类型，区分 finalize 失败和 rollback 失败。
///
/// 上层遇到 `RollbackFailed` 时不能把 transaction 清掉，必须保留给下次恢复。
#[derive(Debug, thiserror::Error)]
pub enum GitFinalizeError {
    #[error("finalize failed: {0}")]
    FinalizeFailed(#[from] crate::Error),
    #[error("finalize failed ({finalize}), rollback also failed: {rollback}")]
    RollbackFailed {
        finalize: String,
        rollback: String,
    },
}

/// #644 评论 5475805198 第2节：应用 Git metadata 变更到 live。
///
/// 在 `SaveTransaction.commit()` 成功后调用。失败时自动 rollback Git metadata。
/// 成功后调用方应调用 `SaveTransaction::finish()` 清理事务。
///
/// #644 评论 5476546134 第4节：返回 `GitFinalizeError`，区分 finalize 失败和 rollback 失败。
/// 上层遇到 `RollbackFailed` 时必须保留 transaction 目录。
pub fn commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
) -> std::result::Result<(), GitFinalizeError> {
    // 调用内部 finalize，失败时 rollback Git metadata。
    if let Err(e) = finalize_git_repo_metadata_inner(live_root, staging_root, seed_state) {
        log::warn!(
            "commit_git_finalize: finalize failed ({}), rolling back Git metadata",
            e
        );
        if let Err(rb_err) = rollback_git_finalize(live_root, snapshot) {
            let rb_msg = rb_err.to_string();
            log::warn!(
                "commit_git_finalize: rollback also failed: {} (original error: {})",
                rb_msg,
                e
            );
            return Err(GitFinalizeError::RollbackFailed {
                finalize: e.to_string(),
                rollback: rb_msg,
            });
        }
        return Err(GitFinalizeError::FinalizeFailed(e));
    }
    Ok(())
}

/// #644 评论 5475805198 第2节 + #644 评论 5476546134 第4节：
/// 从快照回滚 Git metadata。
///
/// 恢复 HEAD、index、refs 到快照时的状态。
/// #644 评论 5476546134 第4节：所有错误直接 `?` 传播，不再吞掉。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub fn rollback_git_finalize(live_root: &Path, snapshot: &GitMetadataSnapshot) -> Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    // 恢复 index。
    match &snapshot.index {
        IndexSnapshot::Bytes(bytes) => {
            let index_path = live_root.join(".git").join("index");
            crate::storage::atomic_write_bytes(&index_path, bytes)?;
        }
        IndexSnapshot::Missing => {
            // index 原本不存在。如果 finalize 新建了 index，删除它。
            let index_path = live_root.join(".git").join("index");
            if index_path.exists() {
                fs::remove_file(&index_path)?;
            }
        }
    }

    // 如果 repo 原本不存在，rollback 需要删除 .git 目录。
    if !snapshot.repo_existed {
        let live_git = live_root.join(".git");
        if live_git.exists() {
            fs::remove_dir_all(&live_git)?;
        }
        return Ok(());
    }

    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "rollback_git_finalize: open live repo: {e}"
        )))
    })?;

    // 恢复 HEAD。
    match &snapshot.head {
        RefSnapshot::DidNotExist => {
            // HEAD 不应存在（理论上不会发生，Git repo 必有 HEAD）。
            if let Ok(mut reference) = live_repo.find_reference("HEAD") {
                reference.delete().map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: delete HEAD: {e}"
                    )))
                })?;
            }
        }
        RefSnapshot::Existed { oid } => {
            let oid = git2::Oid::from_str(oid).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "rollback_git_finalize: invalid HEAD oid: {e}"
                )))
            })?;
            live_repo
                .reference("HEAD", oid, true, "rollback: restore HEAD")
                .map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: restore HEAD: {e}"
                    )))
                })?;
        }
        RefSnapshot::Symbolic { target } => {
            live_repo
                .reference_symbolic("HEAD", target, true, "rollback: restore HEAD")
                .map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: restore HEAD symbolic: {e}"
                    )))
                })?;
        }
    }

    // 恢复所有 refs。
    for (ref_name, ref_snapshot) in &snapshot.refs {
        match ref_snapshot {
            RefSnapshot::DidNotExist => {
                // finalize 新建了此 ref，删除。
                if let Ok(mut reference) = live_repo.find_reference(ref_name) {
                    reference.delete().map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rollback_git_finalize: delete {}: {e}",
                            ref_name
                        )))
                    })?;
                }
            }
            RefSnapshot::Existed { oid } => {
                let oid = git2::Oid::from_str(oid).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: invalid oid for {}: {e}",
                        ref_name
                    )))
                })?;
                live_repo
                    .reference(ref_name, oid, true, "rollback: restore ref")
                    .map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rollback_git_finalize: restore {}: {e}",
                            ref_name
                        )))
                    })?;
            }
            RefSnapshot::Symbolic { target } => {
                live_repo
                    .reference_symbolic(ref_name, target, true, "rollback: restore ref")
                    .map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rollback_git_finalize: restore {} symbolic: {e}",
                            ref_name
                        )))
                    })?;
            }
        }
    }

    Ok(())
}

/// #644 评论 5475805198 第2节：崩溃恢复。
///
/// 进程在 `SaveTransaction.commit()` 和 `commit_git_finalize()` 之间退出后，
/// 下次启动调用本函数尝试完成 Git finalize。
///
/// 成功：清理事务目录。
/// 失败：回滚文件（由调用方处理 SaveTransaction rollback）。
pub fn recover_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
) -> Result<()> {
    // 尝试完成 Git finalize。
    match finalize_git_repo_metadata_inner(live_root, staging_root, seed_state) {
        Ok(()) => {
            log::info!("recover_git_finalize: successfully completed pending Git finalize");
            Ok(())
        }
        Err(e) => {
            log::warn!(
                "recover_git_finalize: finalize failed ({}), rolling back Git metadata",
                e
            );
            rollback_git_finalize(live_root, snapshot)?;
            Err(e)
        }
    }
}

/// 包装函数，供 `sync_ops.rs` 调用。
///
/// #644 评论 5476546134 第4节：返回 `GitFinalizeError`，上层遇到 `RollbackFailed`
/// 时必须保留 transaction 目录。
pub fn try_commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: Option<&GitSeedState>,
    snapshot: Option<&GitMetadataSnapshot>,
) -> std::result::Result<(), GitFinalizeError> {
    let Some(state) = seed_state else {
        return Ok(());
    };
    let Some(snap) = snapshot else {
        return Err(GitFinalizeError::FinalizeFailed(
            crate::Error::Io(std::io::Error::other(
                "missing GitMetadataSnapshot for Git backend",
            )),
        ));
    };
    commit_git_finalize(live_root, staging_root, state, snap)
}

// ── 内部 finalize 实现 ──

/// 内部 finalize 实现，不含 rollback（由调用方处理）。
#[allow(clippy::too_many_lines)]
fn finalize_git_repo_metadata_inner(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
) -> Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    let staging_git_dir = staging_root.join(".git");
    if !staging_git_dir.exists() {
        return Ok(());
    }

    let staging_repo = git2::Repository::open(staging_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: failed to open staging repo: {e}"
        )))
    })?;

    let staging_head = staging_repo.head().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: staging HEAD missing: {e}"
        )))
    })?;
    let new_oid = staging_head.target().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(
            "finalize: staging HEAD is unborn (no target OID)",
        ))
    })?;

    let staging_odb = staging_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("finalize: staging odb: {e}")))
    })?;

    let branch_name = if let Some(name) = staging_head.shorthand() {
        name.to_string()
    } else {
        "main".to_string()
    };

    match seed_state {
        GitSeedState::NotGitRepo => finalize_not_git_repo(
            live_root,
            staging_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            &branch_name,
        ),
        GitSeedState::Unborn { head_ref } => {
            finalize_unborn(live_root, &staging_repo, &staging_odb, new_oid, head_ref)
        }
        GitSeedState::Existing { head_ref, head_oid } => finalize_existing(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            head_ref,
            *head_oid,
        ),
        GitSeedState::Detached { head_oid } => {
            finalize_detached(live_root, &staging_repo, &staging_odb, new_oid, *head_oid)
        }
    }
}

/// finalize 路径 1：live 原本不是 Git repo。
///
/// #644 评论 5475413230 第2节：原子性改进。
/// #644 评论 5475805198 第4节：RAII 守卫保证临时目录清理。
fn finalize_not_git_repo(
    live_root: &Path,
    staging_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    branch_name: &str,
) -> Result<()> {
    let staging_git = staging_root.join(".git");
    let live_git = live_root.join(".git");

    // #644 评论 5475805198 第4节：RAII 守卫，任何返回路径都删除临时目录。
    let tmp_id = uuid::Uuid::new_v4().to_string();
    let tmp_git = live_root.join(format!(".git.sujian-tmp-{}", tmp_id));
    let mut _guard = TmpDirGuard::new(tmp_git.clone());

    copy_dir_recursive(&staging_git, _guard.path())?;

    let tmp_repo = git2::Repository::open(_guard.path()).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: open tmp repo: {e}"
        )))
    })?;
    let tmp_odb = tmp_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: tmp odb: {e}"
        )))
    })?;

    import_missing_objects(staging_odb, &tmp_odb)?;

    if !tmp_odb.exists(new_oid) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: new_oid {} not found in tmp after import",
            new_oid
        ))));
    }

    let ref_name = format!("refs/heads/{}", branch_name);
    tmp_repo
        .reference(&ref_name, new_oid, true, "sync: init branch from staging")
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_not_git_repo: update-ref {} failed: {}",
                ref_name, e
            )))
        })?;

    update_live_index(&tmp_repo, staging_repo, new_oid)?;

    if live_git.exists() {
        return Err(crate::Error::Io(std::io::Error::other(
            "finalize_not_git_repo: live .git appeared during finalize (concurrent modification)",
        )));
    }

    // 成功：rename 临时目录到 .git，取消守卫自动删除。
    let guard_path = _guard.disarm();
    fs::rename(&guard_path, &live_git).map_err(|e| {
        // rename 失败，手动清理。
        let _ = fs::remove_dir_all(&guard_path);
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: rename tmp -> .git failed: {e}"
        )))
    })?;

    Ok(())
}

/// finalize 路径 2：live 是 unborn repo。
///
/// #644 评论 5475805198 第3节：使用 `find_reference("HEAD")` 读取未 resolve 的 HEAD，
/// 确认 `symbolic_target() == seed head_ref` 且目标 branch ref 仍不存在。
fn finalize_unborn(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
) -> Result<()> {
    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_unborn: open live repo: {e}"
        )))
    })?;
    let live_odb = live_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_unborn: live odb: {e}"
        )))
    })?;

    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_unborn: new_oid {} not found in live after import",
            new_oid
        ))));
    }

    update_live_index(&live_repo, staging_repo, new_oid)?;
    sync_remote_refs(&live_repo, staging_repo)?;

    // #644 评论 5475805198 第3节：使用 find_reference("HEAD") 读取未 resolve 的 HEAD。
    // head() 会 resolve symbolic ref，unborn 时返回 UnbornBranch 错误，
    // 导致 if let Ok(...) 跳过校验。
    let raw_head = live_repo.find_reference("HEAD").map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_unborn: HEAD reference not found: {e}"
        )))
    })?;

    // 确认 HEAD 仍是 symbolic 且指向 seed 时的同一个 branch。
    if let Some(sym_target) = raw_head.symbolic_target() {
        if sym_target != head_ref {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "finalize_unborn: HEAD changed from {} to {} during finalize",
                head_ref, sym_target
            ))));
        }
    } else {
        return Err(crate::Error::Io(std::io::Error::other(
            "finalize_unborn: HEAD is no longer symbolic (concurrent modification)",
        )));
    }

    // 确认目标 branch ref 仍不存在（unborn 状态未变）。
    if live_repo.find_reference(head_ref).is_ok() {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_unborn: branch ref {} already exists (concurrent modification)",
            head_ref
        ))));
    }

    live_repo
        .reference(head_ref, new_oid, false, "sync: create branch from staging")
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_unborn: create-ref {} failed: {}",
                head_ref, e
            )))
        })?;

    Ok(())
}

/// finalize 路径 3：live 已有提交的 repo。
///
/// #644 评论 5475805198 第3节：除 head_ref CAS 外，确认 HEAD 仍指向同一 branch。
fn finalize_existing(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    base_oid: git2::Oid,
) -> Result<()> {
    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_existing: open live repo: {e}"
        )))
    })?;
    let live_odb = live_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_existing: live odb: {e}"
        )))
    })?;

    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_existing: new_oid {} not found in live after import",
            new_oid
        ))));
    }

    update_live_index(&live_repo, staging_repo, new_oid)?;
    sync_remote_refs(&live_repo, staging_repo)?;

    // #644 评论 5475805198 第3节：确认 HEAD 仍指向 seed 时同一个 branch。
    // 防止用户切换到别的 branch 后还偷偷更新旧 branch。
    if let Ok(raw_head) = live_repo.find_reference("HEAD") {
        if let Some(sym_target) = raw_head.symbolic_target() {
            if sym_target != head_ref {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "finalize_existing: HEAD now points to {} but seed was {}",
                    sym_target, head_ref
                ))));
            }
        }
    }

    live_repo
        .reference_matching(
            head_ref,
            new_oid,
            false,
            base_oid,
            "sync: finalize git repo metadata after full sync",
        )
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: update-ref {} failed (CAS old={} new={}): {}",
                head_ref, base_oid, new_oid, e
            )))
        })?;

    Ok(())
}

/// finalize 路径 4：live 是 detached HEAD。
///
/// #644 评论 5475805198 第3节：使用 `reference_matching("HEAD", ...)` 做真正的 CAS。
/// 当前代码用 `reference("HEAD", new_oid, true, ...)` 会强制覆盖并发变化。
fn finalize_detached(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    base_oid: git2::Oid,
) -> Result<()> {
    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_detached: open live repo: {e}"
        )))
    })?;
    let live_odb = live_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_detached: live odb: {e}"
        )))
    })?;

    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_detached: new_oid {} not found in live after import",
            new_oid
        ))));
    }

    update_live_index(&live_repo, staging_repo, new_oid)?;

    // #644 评论 5475805198 第3节：使用 reference_matching 做真正的 CAS。
    // current OID 不一致会返回 modified，而不是覆盖。
    live_repo
        .reference_matching(
            "HEAD",
            new_oid,
            false,
            base_oid,
            "sync: finalize detached HEAD after full sync",
        )
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_detached: update HEAD failed (CAS old={} new={}): {}",
                base_oid, new_oid, e
            )))
        })?;

    Ok(())
}

// ── 内部辅助函数（从 git_staging.rs 移入） ──

/// 从 staging ODB 导入 live ODB 缺失的对象。
fn import_missing_objects(staging_odb: &git2::Odb, live_odb: &git2::Odb) -> Result<()> {
    let mut missing_oids: Vec<git2::Oid> = Vec::new();
    staging_odb
        .foreach(|oid| {
            if !live_odb.exists(*oid) {
                missing_oids.push(*oid);
            }
            true
        })
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "import_missing_objects: foreach: {e}"
            )))
        })?;

    for oid in &missing_oids {
        let obj = staging_odb.read(*oid).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "import_missing_objects: read {}: {}",
                oid, e
            )))
        })?;
        live_odb.write(obj.kind(), obj.data()).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "import_missing_objects: write {}: {}",
                oid, e
            )))
        })?;
    }

    Ok(())
}

/// 同步 staging 的 remote-tracking refs 到 live。
fn sync_remote_refs(live_repo: &git2::Repository, staging_repo: &git2::Repository) -> Result<()> {
    if let Ok(staging_refs) = staging_repo.references() {
        for reference in staging_refs.flatten() {
            let Some(name) = reference.name() else {
                continue;
            };
            if !name.starts_with("refs/remotes/") {
                continue;
            }
            let Some(target) = reference.target() else {
                continue;
            };
            live_repo
                .reference(
                    name,
                    target,
                    true,
                    "sync: update remote-tracking ref from staging",
                )
                .map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "sync_remote_refs: update {} failed: {}",
                        name, e
                    )))
                })?;
        }
    }
    Ok(())
}

/// 更新 live 的 index 为 staging HEAD 对应的 tree（不 checkout 工作区）。
fn update_live_index(
    live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
) -> Result<()> {
    let new_commit = staging_repo.find_commit(new_oid).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: find commit: {e}"
        )))
    })?;
    let new_tree = new_commit.tree().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: find tree: {e}"
        )))
    })?;

    let mut live_index = live_repo.index().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: get index: {e}"
        )))
    })?;
    live_index.read_tree(&new_tree).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: read_tree: {e}"
        )))
    })?;
    live_index.write().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "update_live_index: write: {e}"
        )))
    })?;

    Ok(())
}

/// 递归复制目录（用于复制 .git/）。
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn prepare_snapshot_non_repo() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();

        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();

        let snapshot = prepare_git_finalize(&live, &GitSeedState::NotGitRepo, &staging).unwrap();
        assert!(matches!(snapshot.head, RefSnapshot::DidNotExist));
        assert!(snapshot.refs.is_empty());
        assert!(matches!(snapshot.index, IndexSnapshot::Missing));
        assert!(!snapshot.repo_existed);
    }

    #[test]
    fn prepare_snapshot_unborn_repo() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        let _repo = git2::Repository::init(&live).unwrap();

        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();

        let seed = GitSeedState::Unborn {
            head_ref: "refs/heads/main".to_string(),
        };
        let snapshot = prepare_git_finalize(&live, &seed, &staging).unwrap();
        assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
        assert!(snapshot.refs.contains_key("refs/heads/main"));
        assert!(snapshot.repo_existed);
    }

    #[test]
    fn prepare_snapshot_existing_repo() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.txt"), "hello").unwrap();

        let repo = git2::Repository::init(&live).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        let commit_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();

        let seed = GitSeedState::Existing {
            head_ref: "refs/heads/main".to_string(),
            head_oid: commit_oid,
        };
        let snapshot = prepare_git_finalize(&live, &seed, &staging).unwrap();
        assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
        assert!(matches!(snapshot.index, IndexSnapshot::Bytes(_)));
        assert!(snapshot.refs.contains_key("refs/heads/main"));
        assert!(snapshot.repo_existed);
    }

    #[test]
    fn rollback_restores_index() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.txt"), "hello").unwrap();

        let repo = git2::Repository::init(&live).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();

        let seed = GitSeedState::Existing {
            head_ref: "refs/heads/main".to_string(),
            head_oid: git2::Oid::zero(),
        };
        let snapshot = prepare_git_finalize(&live, &seed, &staging).unwrap();
        let original_index = match &snapshot.index {
            IndexSnapshot::Bytes(b) => b.clone(),
            _ => panic!("expected IndexSnapshot::Bytes"),
        };

        // 修改 index。
        fs::write(live.join("b.txt"), "new").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("b.txt")).unwrap();
        index.write().unwrap();

        // Rollback 应恢复原始 index。
        rollback_git_finalize(&live, &snapshot).unwrap();
        let restored_index = fs::read(live.join(".git").join("index")).unwrap();
        assert_eq!(restored_index, original_index);
    }
}
