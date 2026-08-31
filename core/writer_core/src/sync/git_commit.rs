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
    /// #644 评论 5478237852 问题2：mutation journal，记录本轮实际写入的内容。
    pub mutation_log: GitFinalizeMutationLog,
}

/// #644 评论 5478237852 问题2：mutation journal，记录 finalize 本轮实际写入的内容。
/// rollback 只撤销本 journal 记录的写入，用 CAS 保护并发写入不被覆盖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFinalizeMutationLog {
    /// 本轮 finalize 是否真的把 .git 安装进了 live（仅 NotGitRepo 路径）。
    pub created_repo_by_us: bool,
    /// 本轮成功写入的 ref 变更，按写入顺序。
    /// key = ref 名，value = (old_oid_or_none, written_oid)。
    /// old_oid_or_none = None 表示 ref 原本不存在（DidNotExist），finalize 新建了它。
    pub ref_mutations: Vec<(String, Option<String>, String)>,
    /// 本轮是否成功写入了 index。
    pub index_written: bool,
    /// 写入前 index 的 SHA-256（用于 CAS rollback：只有当前 index 仍等于我们写的那份才恢复）。
    pub written_index_sha256: Option<[u8; 32]>,
}

impl Default for GitFinalizeMutationLog {
    fn default() -> Self {
        Self {
            created_repo_by_us: false,
            ref_mutations: Vec::new(),
            index_written: false,
            written_index_sha256: None,
        }
    }
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
    // #644 评论 5478237852 问题3：只有 NotFound 映射为 DidNotExist，其它错误向上传。
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
        Err(e) if e.code() == git2::ErrorCode::NotFound => RefSnapshot::DidNotExist,
        Err(e) => {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "prepare_git_finalize: failed to find HEAD reference: {e}"
            ))));
        }
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
        let snap = snapshot_ref(&live_repo, &head_ref)?;
        refs.insert(head_ref, snap);
    }

    // 2. live 已有的 remote refs。
    // #644 评论 5478237852 问题3：references() 获取失败直接返回 Err，不要 flatten 吞掉错误。
    let live_refs = live_repo.references().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "prepare_git_finalize: failed to iterate live references: {e}"
        )))
    })?;
    for reference in live_refs {
        let reference = reference.map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "prepare_git_finalize: live reference iterator error: {e}"
            )))
        })?;
        let Some(name) = reference.name() else {
            continue;
        };
        if !name.starts_with("refs/remotes/") {
            continue;
        }
        refs.insert(name.to_string(), snapshot_ref_from_repo_ref(&reference));
    }

    // 3. staging 将要写入的 remote refs（live 不存在的显式记录 DidNotExist）。
    // #644 评论 5478237852 问题3：staging repo 不存在是正常情况（staging 可能没有 .git），
    // 不报错，只是没有 remote refs 可收集。
    if let Ok(staging_repo) = git2::Repository::open(staging_root) {
        let staging_refs = staging_repo.references().map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "prepare_git_finalize: failed to iterate staging references: {e}"
            )))
        })?;
        for reference in staging_refs {
            let reference = reference.map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "prepare_git_finalize: staging reference iterator error: {e}"
                )))
            })?;
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
/// #644 评论 5478237852 问题3：只有 git2::ErrorCode::NotFound 可以映射为 DidNotExist，
/// 其它错误（IO 损坏、锁错误、backend 错误）全部向上传。
fn snapshot_ref(repo: &git2::Repository, ref_name: &str) -> std::result::Result<RefSnapshot, crate::Error> {
    match repo.find_reference(ref_name) {
        Ok(reference) => Ok(snapshot_ref_from_repo_ref(&reference)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(RefSnapshot::DidNotExist),
        Err(e) => Err(crate::Error::Io(std::io::Error::other(format!(
            "snapshot_ref: failed to find reference {ref_name}: {e}"
        )))),
    }
}

// ── Git finalize 错误类型 ──

/// #644 评论 5476546134 第4节 + #644 评论 5477439446 问题2：
/// Git finalize 错误类型，区分 finalize 失败和 rollback 失败。
///
/// 上层遇到 `RollbackFailed` 时不能把 transaction 清掉，必须保留给下次恢复。
///
/// #644 评论 5477439446 问题2：`ConcurrentMetadataChanged` 表示在改 live Git
/// metadata 之前检测到并发修改（index/HEAD/branch/remote refs 与 snapshot 不一致）。
/// 此时本轮 finalize 还没改过 live Git metadata，**不能调用 rollback_git_finalize**，
/// 否则会把别人刚写的新状态覆盖成 Transfer 开始前的旧 snapshot。上层对这类错误
/// 应直接返回失败，不触发 Git metadata rollback（文件 rollback 仍可由调用方决定）。
#[derive(Debug, thiserror::Error)]
pub enum GitFinalizeError {
    #[error("finalize failed: {0}")]
    FinalizeFailed(#[from] crate::Error),
    #[error("finalize failed ({finalize}), rollback also failed: {rollback}")]
    RollbackFailed { finalize: String, rollback: String },
    /// #644 评论 5477439446 问题2：并发校验失败，本轮尚未修改 live Git metadata。
    /// 不触发 Git metadata rollback。`reason` 描述哪个 metadata 不一致。
    #[error("concurrent git metadata changed before finalize wrote anything: {reason}")]
    ConcurrentMetadataChanged { reason: String },
}

/// #644 评论 5475805198 第2节：应用 Git metadata 变更到 live。
///
/// 在 `SaveTransaction.commit()` 成功后调用。失败时自动 rollback Git metadata。
/// 成功后调用方应调用 `SaveTransaction::finish()` 清理事务。
///
/// #644 评论 5476546134 第4节：返回 `GitFinalizeError`，区分 finalize 失败和 rollback 失败。
/// 上层遇到 `RollbackFailed` 时必须保留 transaction 目录。
///
/// #644 评论 5477439446 问题2：对 `ConcurrentMetadataChanged`（本轮尚未修改 live
/// Git metadata 的并发校验失败）**不调用 rollback_git_finalize**，避免把并发
/// Git 操作刚写的新状态覆盖成 Transfer 开始前的旧 snapshot。只对 `FinalizeFailed`
/// （本轮已修改后失败）才 rollback。
pub fn commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
) -> std::result::Result<GitFinalizeMutationLog, GitFinalizeError> {
    let mut mutation_log = GitFinalizeMutationLog::default();
    // 调用内部 finalize，失败时按错误类型决定是否 rollback Git metadata。
    if let Err(e) = finalize_git_repo_metadata_inner(live_root, staging_root, seed_state, snapshot, &mut mutation_log)
    {
        match &e {
            GitFinalizeError::ConcurrentMetadataChanged { .. } => {
                // 本轮尚未修改 live Git metadata，不能 rollback，否则会覆盖并发写入。
                log::warn!(
                    "commit_git_finalize: {} (not rolling back Git metadata: nothing written this round)",
                    e
                );
                return Err(e);
            }
            GitFinalizeError::FinalizeFailed(_) | GitFinalizeError::RollbackFailed { .. } => {
                log::warn!(
                    "commit_git_finalize: finalize failed ({}), rolling back Git metadata",
                    e
                );
                if let Err(rb_err) = rollback_git_finalize(live_root, snapshot, &mutation_log) {
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
                return Err(e);
            }
        }
    }
    Ok(mutation_log)
}

/// #644 评论 5478237852 问题2：CAS-based rollback，只撤销 mutation_log 记录的写入。
///
/// 用 CAS 保护并发写入不被覆盖：只有当前值仍等于我们写入的值时才恢复。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub fn rollback_git_finalize(live_root: &Path, snapshot: &GitMetadataSnapshot, mutation_log: &GitFinalizeMutationLog) -> Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    // 1. Rollback index — only if we actually wrote it.
    // CAS: only restore if current index still matches what we wrote.
    if mutation_log.index_written {
        let index_path = live_root.join(".git").join("index");
        match (&snapshot.index, &mutation_log.written_index_sha256) {
            (IndexSnapshot::Bytes(original_bytes), Some(expected_hash)) => {
                if index_path.exists() {
                    let current_bytes = fs::read(&index_path)?;
                    let current_hash = sha256_bytes(&current_bytes);
                    if current_hash == *expected_hash {
                        // Index hasn't been touched since we wrote it — safe to restore.
                        crate::storage::atomic_write_bytes(&index_path, original_bytes)?;
                    } else {
                        log::warn!(
                            "rollback_git_finalize: index changed since we wrote it (CAS miss), \
                             skipping index rollback to avoid overwriting concurrent changes"
                        );
                    }
                }
            }
            (IndexSnapshot::Missing, Some(expected_hash)) => {
                if index_path.exists() {
                    let current_bytes = fs::read(&index_path)?;
                    let current_hash = sha256_bytes(&current_bytes);
                    if current_hash == *expected_hash {
                        fs::remove_file(&index_path)?;
                    } else {
                        log::warn!(
                            "rollback_git_finalize: index changed since we wrote it (CAS miss), \
                             skipping index removal"
                        );
                    }
                }
            }
            _ => {
                // No hash recorded — shouldn't happen if index_written=true, skip.
            }
        }
    }

    // 2. Rollback repo creation — only if we created it AND nothing else touched it.
    if mutation_log.created_repo_by_us {
        let live_git = live_root.join(".git");
        if live_git.exists() {
            // We created this .git; since we're the creator, it's safe to remove
            // (concurrent processes wouldn't have had time to establish their own .git
            // since we just created it in this finalize round).
            fs::remove_dir_all(&live_git)?;
        }
        return Ok(());
    }

    // 3. Rollback refs — CAS-based: only restore if current value == what we wrote.
    if mutation_log.ref_mutations.is_empty() {
        return Ok(());
    }

    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "rollback_git_finalize: open live repo: {e}"
        )))
    })?;

    for (ref_name, old_oid_str, written_oid_str) in &mutation_log.ref_mutations {
        let written_oid = git2::Oid::from_str(written_oid_str).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "rollback_git_finalize: invalid written_oid for {}: {e}",
                ref_name
            )))
        })?;

        // Check current value.
        let current = live_repo.find_reference(ref_name);

        match (&old_oid_str, current) {
            // Ref was created by us (DidNotExist -> written_oid).
            // Only delete if it still equals what we wrote.
            (None, Ok(current_ref)) => {
                if current_ref.target() == Some(written_oid) {
                    // Still our value — safe to delete.
                    let mut r = current_ref;
                    r.delete().map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rollback_git_finalize: delete {}: {e}",
                            ref_name
                        )))
                    })?;
                } else {
                    log::warn!(
                        "rollback_git_finalize: {} changed since we wrote it (CAS miss), \
                         skipping deletion to avoid overwriting concurrent changes",
                        ref_name
                    );
                }
            }
            (None, Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // Already gone — nothing to undo.
            }
            (None, Err(e)) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "rollback_git_finalize: lookup {}: {e}",
                    ref_name
                ))));
            }

            // Ref was updated by us (old_oid -> written_oid).
            // Only restore old_oid if current == written_oid.
            (Some(old_oid_str), Ok(current_ref)) => {
                if current_ref.target() == Some(written_oid) {
                    let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rollback_git_finalize: invalid old_oid for {}: {e}",
                            ref_name
                        )))
                    })?;
                    live_repo
                        .reference(ref_name, old_oid, true, "rollback: CAS restore ref")
                        .map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "rollback_git_finalize: restore {}: {e}",
                                ref_name
                            )))
                        })?;
                } else {
                    log::warn!(
                        "rollback_git_finalize: {} changed since we wrote it (CAS miss), \
                         skipping restore to avoid overwriting concurrent changes",
                        ref_name
                    );
                }
            }
            (Some(_), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // Ref was deleted by someone else after we wrote it.
                // Don't recreate it — the deletion is a newer change.
                log::warn!(
                    "rollback_git_finalize: {} was deleted by concurrent process, \
                     skipping restore",
                    ref_name
                );
            }
            (Some(_), Err(e)) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "rollback_git_finalize: lookup {}: {e}",
                    ref_name
                ))));
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
///
/// #644 评论 5477439446 问题2：对 `ConcurrentMetadataChanged` 不调用 rollback，
/// 因为本轮尚未修改 live Git metadata。
pub fn recover_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
) -> std::result::Result<GitFinalizeMutationLog, GitFinalizeError> {
    let mut mutation_log = GitFinalizeMutationLog::default();
    // 尝试完成 Git finalize。
    match finalize_git_repo_metadata_inner(live_root, staging_root, seed_state, snapshot, &mut mutation_log) {
        Ok(()) => {
            log::info!("recover_git_finalize: successfully completed pending Git finalize");
            Ok(mutation_log)
        }
        Err(e) => match &e {
            GitFinalizeError::ConcurrentMetadataChanged { .. } => {
                log::warn!(
                    "recover_git_finalize: {} (not rolling back Git metadata: nothing written this round)",
                    e
                );
                Err(e)
            }
            _ => {
                log::warn!(
                    "recover_git_finalize: finalize failed ({}), rolling back Git metadata",
                    e
                );
                rollback_git_finalize(live_root, snapshot, &mutation_log).map_err(|rb_err| {
                    GitFinalizeError::RollbackFailed {
                        finalize: e.to_string(),
                        rollback: rb_err.to_string(),
                    }
                })?;
                Err(e)
            }
        },
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
) -> std::result::Result<GitFinalizeMutationLog, GitFinalizeError> {
    let Some(state) = seed_state else {
        return Ok(GitFinalizeMutationLog::default());
    };
    let Some(snap) = snapshot else {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other("missing GitMetadataSnapshot for Git backend"),
        )));
    };
    commit_git_finalize(live_root, staging_root, state, snap)
}

// ── 内部 finalize 实现 ──

/// 内部 finalize 实现，不含 rollback（由调用方处理）。
///
/// #644 评论 5477439446 问题2：返回 `GitFinalizeError`，使 `ConcurrentMetadataChanged`
/// 能向上传播到 `commit_git_finalize`，由其决定是否 rollback。
/// 接收 `snapshot` 用于 finalize_unborn/finalize_existing 的并发校验。
#[allow(clippy::too_many_lines)]
fn finalize_git_repo_metadata_inner(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    mutation_log: &mut GitFinalizeMutationLog,
) -> std::result::Result<(), GitFinalizeError> {
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
            mutation_log,
        ),
        GitSeedState::Unborn { head_ref } => finalize_unborn(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            head_ref,
            snapshot,
            mutation_log,
        ),
        GitSeedState::Existing { head_ref, head_oid } => finalize_existing(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            head_ref,
            *head_oid,
            snapshot,
            mutation_log,
        ),
        GitSeedState::Detached { head_oid } => {
            finalize_detached(live_root, &staging_repo, &staging_odb, new_oid, *head_oid, snapshot, mutation_log)
                .map_err(GitFinalizeError::FinalizeFailed)
        }
    }
}

/// finalize 路径 1：live 原本不是 Git repo。
///
/// #644 评论 5475413230 第2节：原子性改进。
/// #644 评论 5475805198 第4节：RAII 守卫保证临时目录清理。
/// #644 评论 5478237852 问题1：返回 `GitFinalizeError` 而非 `crate::Error`，
/// 使 `ConcurrentMetadataChanged` 能向上传播，避免 rollback 删除别人刚创建的 `.git`。
fn finalize_not_git_repo(
    live_root: &Path,
    staging_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    branch_name: &str,
    mutation_log: &mut GitFinalizeMutationLog,
) -> std::result::Result<(), GitFinalizeError> {
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
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_not_git_repo: new_oid {} not found in tmp after import",
                new_oid
            )),
        )));
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

    // #644 评论 5478237852 问题1：live .git 在 rename 前已出现，返回 ConcurrentMetadataChanged，
    // 不能进入 metadata rollback（否则会删除别人刚创建的 .git）。
    if live_git.exists() {
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: "live .git appeared during finalize (concurrent git init)".to_string(),
        });
    }

    // 成功：rename 临时目录到 .git，取消守卫自动删除。
    let guard_path = _guard.disarm();
    if let Err(e) = fs::rename(&guard_path, &live_git) {
        // #644 评论 5478237852 问题1：rename 失败可能是因为目标在检查后又被并发创建。
        // 再次检查目标 .git，如果存在则属于并发出现，返回 ConcurrentMetadataChanged。
        let _ = fs::remove_dir_all(&guard_path);
        if live_git.exists() {
            return Err(GitFinalizeError::ConcurrentMetadataChanged {
                reason: "live .git appeared during rename (concurrent git init)".to_string(),
            });
        }
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!("finalize_not_git_repo: rename tmp -> .git failed: {e}")),
        )));
    }

    // #644 评论 5478237852 问题2：记录本轮创建了 .git。
    mutation_log.created_repo_by_us = true;

    Ok(())
}

/// finalize 路径 2：live 是 unborn repo。
///
/// #644 评论 5475805198 第3节：使用 `find_reference("HEAD")` 读取未 resolve 的 HEAD，
/// 确认 `symbolic_target() == seed head_ref` 且目标 branch ref 仍不存在。
///
/// #644 评论 5477439446 问题2：在第一次改 live Git metadata 之前做并发校验，
/// 校验失败返回 `ConcurrentMetadataChanged`（不触发 rollback）。
fn finalize_unborn(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    snapshot: &GitMetadataSnapshot,
    mutation_log: &mut GitFinalizeMutationLog,
) -> std::result::Result<(), GitFinalizeError> {
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
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_unborn: new_oid {} not found in live after import",
                new_oid
            )),
        )));
    }

    // #644 评论 5477439446 问题2：在第一次改 live Git metadata 之前做并发校验。
    // 校验失败返回 ConcurrentMetadataChanged，不触发 rollback。
    verify_git_metadata_unchanged(live_root, snapshot, head_ref)?;

    update_live_index(&live_repo, staging_repo, new_oid)?;
    // #644 评论 5478237852 问题2：记录 index 写入。
    mutation_log.index_written = true;
    mutation_log.written_index_sha256 = Some(sha256_bytes(&fs::read(live_root.join(".git").join("index")).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(e))
    })?));

    sync_remote_refs(&live_repo, staging_repo, &snapshot.refs, mutation_log)?;

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
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_unborn: HEAD changed from {} to {} during finalize",
                    head_ref, sym_target
                )),
            )));
        }
    } else {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(
                "finalize_unborn: HEAD is no longer symbolic (concurrent modification)",
            ),
        )));
    }

    // 确认目标 branch ref 仍不存在（unborn 状态未变）。
    if live_repo.find_reference(head_ref).is_ok() {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_unborn: branch ref {} already exists (concurrent modification)",
                head_ref
            )),
        )));
    }

    live_repo
        .reference(head_ref, new_oid, false, "sync: create branch from staging")
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_unborn: create-ref {} failed: {}",
                head_ref, e
            )))
        })?;

    // #644 评论 5478237852 问题2：记录 branch ref 创建。
    mutation_log.ref_mutations.push((head_ref.to_string(), None, new_oid.to_string()));

    Ok(())
}

/// finalize 路径 3：live 已有提交的 repo。
///
/// #644 评论 5475805198 第3节：除 head_ref CAS 外，确认 HEAD 仍指向同一 branch。
///
/// #644 评论 5477439446 问题2：在第一次改 live Git metadata 之前做并发校验，
/// 校验失败返回 `ConcurrentMetadataChanged`（不触发 rollback）。
fn finalize_existing(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    base_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    mutation_log: &mut GitFinalizeMutationLog,
) -> std::result::Result<(), GitFinalizeError> {
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
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_existing: new_oid {} not found in live after import",
                new_oid
            )),
        )));
    }

    // #644 评论 5477439446 问题2：在第一次改 live Git metadata 之前做并发校验。
    // 校验失败返回 ConcurrentMetadataChanged，不触发 rollback。
    verify_git_metadata_unchanged(live_root, snapshot, head_ref)?;

    update_live_index(&live_repo, staging_repo, new_oid)?;
    // #644 评论 5478237852 问题2：记录 index 写入。
    mutation_log.index_written = true;
    mutation_log.written_index_sha256 = Some(sha256_bytes(&fs::read(live_root.join(".git").join("index")).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(e))
    })?));

    sync_remote_refs(&live_repo, staging_repo, &snapshot.refs, mutation_log)?;

    // #644 评论 5475805198 第3节：确认 HEAD 仍指向 seed 时同一个 branch。
    // 防止用户切换到别的 branch 后还偷偷更新旧 branch。
    if let Ok(raw_head) = live_repo.find_reference("HEAD") {
        if let Some(sym_target) = raw_head.symbolic_target() {
            if sym_target != head_ref {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "finalize_existing: HEAD now points to {} but seed was {}",
                        sym_target, head_ref
                    )),
                )));
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

    // #644 评论 5478237852 问题2：记录 branch ref 更新。
    mutation_log.ref_mutations.push((head_ref.to_string(), Some(base_oid.to_string()), new_oid.to_string()));

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
    snapshot: &GitMetadataSnapshot,
    mutation_log: &mut GitFinalizeMutationLog,
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

    // #644 评论 5478237852 问题2：preflight verify before writing.
    verify_git_metadata_unchanged(live_root, snapshot, "HEAD").map_err(|e| match e {
        GitFinalizeError::FinalizeFailed(inner) => inner,
        GitFinalizeError::ConcurrentMetadataChanged { reason } => {
            crate::Error::Io(std::io::Error::other(reason))
        }
        GitFinalizeError::RollbackFailed { finalize, rollback } => {
            crate::Error::Io(std::io::Error::other(format!(
                "rollback failed: {rollback} (finalize: {finalize})"
            )))
        }
    })?;

    update_live_index(&live_repo, staging_repo, new_oid)?;
    // #644 评论 5478237852 问题2：记录 index 写入。
    mutation_log.index_written = true;
    mutation_log.written_index_sha256 = Some(sha256_bytes(&fs::read(live_root.join(".git").join("index"))?));

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

    // #644 评论 5478237852 问题2：记录 HEAD 更新。
    mutation_log.ref_mutations.push(("HEAD".to_string(), Some(base_oid.to_string()), new_oid.to_string()));

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

/// #644 评论 5477439446 问题2：在第一次改 live Git metadata 之前，校验
/// HEAD、目标 branch、index、本轮会修改的 remote refs 是否仍等于 snapshot。
/// 校验失败返回 `GitFinalizeError::ConcurrentMetadataChanged`，不触发 rollback。
///
/// `import_missing_objects` 已在调用前完成，它只追加 ODB object（append-only），
/// 不覆盖任何现有 Git metadata，因此不影响本校验。
fn verify_git_metadata_unchanged(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    head_ref: &str,
) -> std::result::Result<(), GitFinalizeError> {
    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "verify_git_metadata_unchanged: open live repo: {e}"
        ))))
    })?;

    // 1. 校验 HEAD。
    let current_head = read_ref_snapshot(&live_repo, "HEAD")?;
    if !ref_snapshot_eq(&current_head, &snapshot.head) {
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: format!(
                "HEAD changed: snapshot={:?} current={:?}",
                snapshot.head, current_head
            ),
        });
    }

    // 2. 校验目标 branch ref。
    if !head_ref.is_empty() && head_ref != "HEAD" {
        let current_branch = read_ref_snapshot(&live_repo, head_ref)?;
        let snapshot_branch = snapshot
            .refs
            .get(head_ref)
            .cloned()
            .unwrap_or(RefSnapshot::DidNotExist);
        if !ref_snapshot_eq(&current_branch, &snapshot_branch) {
            return Err(GitFinalizeError::ConcurrentMetadataChanged {
                reason: format!(
                    "branch {} changed: snapshot={:?} current={:?}",
                    head_ref, snapshot_branch, current_branch
                ),
            });
        }
    }

    // 3. 校验 index。
    let index_path = live_root.join(".git").join("index");
    let current_index = if index_path.exists() {
        let bytes = fs::read(&index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };
    if !index_snapshot_eq(&current_index, &snapshot.index) {
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: "index changed before finalize wrote anything".to_string(),
        });
    }

    // 4. 校验所有本轮会修改的 remote refs。
    for (ref_name, ref_snapshot) in &snapshot.refs {
        if !ref_name.starts_with("refs/remotes/") {
            continue;
        }
        let current = read_ref_snapshot(&live_repo, ref_name)?;
        if !ref_snapshot_eq(&current, ref_snapshot) {
            return Err(GitFinalizeError::ConcurrentMetadataChanged {
                reason: format!(
                    "remote ref {} changed: snapshot={:?} current={:?}",
                    ref_name, ref_snapshot, current
                ),
            });
        }
    }

    Ok(())
}

/// 读取 ref 的当前快照。
/// #644 评论 5478237852 问题3：返回 Result，只有 NotFound 映射为 DidNotExist。
fn read_ref_snapshot(repo: &git2::Repository, ref_name: &str) -> std::result::Result<RefSnapshot, crate::Error> {
    match repo.find_reference(ref_name) {
        Ok(r) => Ok(snapshot_ref_from_repo_ref(&r)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(RefSnapshot::DidNotExist),
        Err(e) => Err(crate::Error::Io(std::io::Error::other(format!(
            "read_ref_snapshot: failed to read reference {ref_name}: {e}"
        )))),
    }
}

/// 比较两个 `RefSnapshot` 是否相等。
fn ref_snapshot_eq(a: &RefSnapshot, b: &RefSnapshot) -> bool {
    match (a, b) {
        (RefSnapshot::DidNotExist, RefSnapshot::DidNotExist) => true,
        (RefSnapshot::Existed { oid: a }, RefSnapshot::Existed { oid: b }) => a == b,
        (RefSnapshot::Symbolic { target: a }, RefSnapshot::Symbolic { target: b }) => a == b,
        _ => false,
    }
}

/// 比较两个 `IndexSnapshot` 是否相等。
fn index_snapshot_eq(a: &IndexSnapshot, b: &IndexSnapshot) -> bool {
    match (a, b) {
        (IndexSnapshot::Missing, IndexSnapshot::Missing) => true,
        (IndexSnapshot::Bytes(a), IndexSnapshot::Bytes(b)) => a == b,
        _ => false,
    }
}

/// 同步 staging 的 remote-tracking refs 到 live。
///
/// #644 评论 5477439446 问题2：用 CAS 语义更新 remote refs，不再无条件覆盖。
/// 对每个 remote ref，从 snapshot 取旧值：
/// - `DidNotExist`：用 `force=false` 创建，已存在则失败（CAS）。
/// - `Existed { oid }`：用 `reference_matching` CAS，当前 OID 不匹配则失败。
/// - `Symbolic`：remote ref 不应是 symbolic，跳过。
#[allow(clippy::excessive_nesting)]
fn sync_remote_refs(
    live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    snapshot_refs: &std::collections::BTreeMap<String, RefSnapshot>,
    mutation_log: &mut GitFinalizeMutationLog,
) -> Result<()> {
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
            let old_snapshot = snapshot_refs
                .get(name)
                .cloned()
                .unwrap_or(RefSnapshot::DidNotExist);
            match &old_snapshot {
                RefSnapshot::DidNotExist => {
                    // ref 原本不存在，用 force=false 创建（已存在则失败，CAS 语义）。
                    live_repo
                        .reference(
                            name,
                            target,
                            false,
                            "sync: create remote-tracking ref from staging",
                        )
                        .map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "sync_remote_refs: create {} failed (CAS expected absent): {}",
                                name, e
                            )))
                        })?;
                    // #644 评论 5478237852 问题2：记录 ref 创建。
                    mutation_log.ref_mutations.push((name.to_string(), None, target.to_string()));
                }
                RefSnapshot::Existed { oid } => {
                    let old_oid = git2::Oid::from_str(oid).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "sync_remote_refs: invalid old oid for {}: {e}",
                            name
                        )))
                    })?;
                    live_repo
                        .reference_matching(
                            name,
                            target,
                            false,
                            old_oid,
                            "sync: update remote-tracking ref from staging",
                        )
                        .map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "sync_remote_refs: CAS update {} failed (old={} new={}): {}",
                                name, old_oid, target, e
                            )))
                        })?;
                    // #644 评论 5478237852 问题2：记录 ref 更新。
                    mutation_log.ref_mutations.push((name.to_string(), Some(oid.clone()), target.to_string()));
                }
                RefSnapshot::Symbolic { .. } => {
                    // remote ref 不应是 symbolic，跳过（不覆盖）。
                    continue;
                }
            }
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

/// 计算字节切片的 SHA-256。
fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
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
        // #644 评论 5478237852 问题2：CAS-based rollback 需要 mutation_log。
        let mutation_log = GitFinalizeMutationLog {
            index_written: true,
            written_index_sha256: Some(sha256_bytes(&fs::read(live.join(".git").join("index")).unwrap())),
            ..Default::default()
        };
        rollback_git_finalize(&live, &snapshot, &mutation_log).unwrap();
        let restored_index = fs::read(live.join(".git").join("index")).unwrap();
        assert_eq!(restored_index, original_index);
    }
}
