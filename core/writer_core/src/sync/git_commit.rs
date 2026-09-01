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
///
/// #644 评论 5480360027：`plan` 是 write-ahead plan，在 `prepare_git_finalize` 时
/// 一次完整生成，随 manifest 原子落盘。finalize 不再依赖只存在内存里的 mutation_log；
/// 成功就 finish，失败/崩溃恢复都只依赖磁盘上的 plan。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFinalizeRecoveryRecord {
    /// seed 时记录的 live Git 仓库状态（序列化友好版本）。
    pub seed_state: SerializableGitSeedState,
    /// finalize 前的 Git metadata 快照。
    pub metadata_snapshot: GitMetadataSnapshot,
    /// #644 评论 5480360027：write-ahead plan，在写 live Git metadata 前完整落盘。
    /// 旧 manifest 中无此字段时反序列化为 `GitFinalizePlan::default()`（向后兼容）。
    #[serde(default)]
    pub plan: GitFinalizePlan,
    /// #644 评论 5478237852 问题2：旧 mutation journal 字段，保留向后兼容。
    /// 新代码使用 `plan`，不再读写此字段。
    #[serde(default)]
    pub mutation_log: GitFinalizeMutationLog,
}

/// #644 评论 5480360027：write-ahead plan，在 `prepare_git_finalize` 时完整生成。
///
/// 与 `GitFinalizeMutationLog` 的根本区别：plan 在写任何 live Git metadata **之前**
/// 就能完整落盘到 manifest，不依赖 finalize 过程中的内存状态。崩溃恢复时只依赖
/// 磁盘上的 plan，根据当前 live 值判断某一步是否真的发生：
/// - current == new_oid → 这一步已执行，反向 CAS 回 old_oid；
/// - current == old_oid → 这一步没执行，无需 rollback；
/// - 两者都不是 → 有并发新状态，不能覆盖。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitFinalizePlan {
    /// 是否会创建 repo（NotGitRepo 路径）。
    pub repo_create: bool,
    /// 计划写入的 index 的 SHA-256。
    /// `None` 表示不会写 index（staging 无 .git 或无 HEAD）。
    /// rollback 时只有当前 index hash 等于此值才恢复 snapshot.index。
    pub new_index_sha256: Option<[u8; 32]>,
    /// 本轮会修改的 ref 变更计划，按计划写入顺序。
    /// key = ref 名，value = (old_oid_or_none, new_oid)。
    /// old_oid_or_none = None 表示 ref 原本不存在（DidNotExist），finalize 会新建。
    pub ref_plans: Vec<(String, Option<String>, String)>,
    /// #644 评论 5481496190 问题3：NotGitRepo 路径的 owner marker（uuid）。
    /// 在 prepare 阶段生成，finalize_not_git_repo 写入 tmp_git/.sujian-sync-owner，
    /// rename 后进入 live .git。rollback 只有 marker 匹配才删除 live .git，
    /// 避免误删外部后来创建的仓库。正常 finalize 完成后删除 marker。
    #[serde(default)]
    pub repo_create_owner: Option<String>,
    /// #644 评论 5484539222 缺陷1 + #644 评论 5486852142 问题1：index.lock 持久 ownership 的 owner uuid。
    /// 在 prepare 阶段生成（当 `new_index_sha256.is_some()` 时），随 manifest 落盘。
    /// finalize/rollback 用 `OwnedIndexLock` 把 `.git/index.lock` 创建为**目录**，
    /// 目录内 `owner` 文件写 owner metadata，`prepared` 文件写目标 index 字节。
    /// 恢复时检查 lock_path 是否是目录 + owner 文件内容判断 ownership：
    /// 属于本轮才清理，不属于本轮（外部 Git regular file lock 或不同事务的目录锁）
    /// 绝不碰。即使进程被 SIGKILL，Drop 不执行，磁盘上 lock 目录仍在，
    /// 恢复时可通过目录类型 + owner 文件判断归属。
    #[serde(default)]
    pub index_lock_owner: Option<String>,
}

/// #644 评论 5478237852 问题2：旧 mutation journal 字段，保留向后兼容。
///
/// 新代码使用 `GitFinalizePlan`。此结构仅用于反序列化旧 manifest。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// #644 评论 5480360027：RAII 守卫，保证临时文件在 drop 时删除。
struct TmpFileGuard(PathBuf);

impl TmpFileGuard {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Drop for TmpFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

// #644 评论 5481496190 问题2：IndexLockGuard 已删除。
// install_index_with_lock 改用 lockfile rename 模型（把内容直接写入 lock 文件，
// rename lock → index 作为提交和解锁），不再需要独立的 RAII lock 守卫。

// ── 公共 API ──

/// #644 评论 5475805198 第2节 + #644 评论 5476546134 第4节 +
/// #644 评论 5480360027：
/// 捕获 finalize 前的 Git metadata 快照，并一次生成完整的 write-ahead plan。
///
/// 在 `SaveTransaction.commit()` 之前调用，`(snapshot, plan)` 随 manifest 原子落盘，
/// 供崩溃恢复使用。plan 在写任何 live Git metadata **之前**就完整持久化，
/// 不依赖 finalize 过程中的内存状态。
///
/// #644 评论 5476546134 第4节：重写快照模型——
/// - `head`：只对应 HEAD 引用本身（symbolic / detached）。
/// - `refs`：所有本轮会修改的 branch/remote refs，包括 staging 将要写入的
///   `refs/remotes/*`（live 不存在的显式记录 `DidNotExist`）。
/// - `index`：`IndexSnapshot::Missing` 或 `IndexSnapshot::Bytes`。
/// - `repo_existed`：finalize 前 live 是否已是 Git repo。
#[allow(clippy::excessive_nesting, clippy::too_many_lines)]
pub fn prepare_git_finalize(
    live_root: &Path,
    seed_state: &GitSeedState,
    staging_root: &Path,
) -> Result<(GitMetadataSnapshot, GitFinalizePlan)> {
    // #644 评论 5480360027：先尝试生成 plan 中与 live 无关的部分（staging HEAD、
    // 目标 index hash、ref_plans 的 new_oid），这样 NotGitRepo 路径也能拿到完整 plan。
    let staging_plan = build_staging_plan(staging_root)?;

    // NotGitRepo 时返回最小快照 + plan，不尝试打开 live repo。
    if matches!(seed_state, GitSeedState::NotGitRepo) {
        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::DidNotExist,
            refs: std::collections::BTreeMap::new(),
            index: IndexSnapshot::Missing,
            repo_existed: false,
        };
        // #644 评论 5480360027：NotGitRepo 路径的 plan 在 build_finalize_plan 中构造。
        let plan = build_finalize_plan(seed_state, &snapshot.refs, &staging_plan, staging_root)?;
        return Ok((snapshot, plan));
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

    // #644 评论 5480360027：根据 snapshot + seed_state + staging_plan 构造完整 plan。
    let plan = build_finalize_plan(seed_state, &refs, &staging_plan, staging_root)?;

    Ok((
        GitMetadataSnapshot {
            head,
            refs,
            index,
            repo_existed: true,
        },
        plan,
    ))
}

/// #644 评论 5480360027：staging 侧 plan 中间结构，只依赖 staging repo + seed_state。
///
/// 在 `prepare_git_finalize` 开头生成，包含目标 index hash 和 ref_plans 的 new_oid 部分。
/// 最终 plan 的 old_oid 由 `build_finalize_plan` 结合 snapshot 补全。
struct StagingPlan {
    /// 计划写入的 index 的 SHA-256（staging HEAD 对应的 tree 生成的 index 字节）。
    /// None 表示 staging 无 .git 或无 HEAD，finalize 不会写 index。
    new_index_sha256: Option<[u8; 32]>,
    /// staging HEAD 的 target OID（hex），用于 ref_plans 的 new_oid。
    /// None 表示 staging 无 .git 或无 HEAD。
    staging_head_oid: Option<String>,
    /// staging HEAD 的 shorthand（branch 名），用于 NotGitRepo 路径的 ref_plans。
    staging_branch_name: String,
}

/// #644 评论 5480360027：从 staging repo 读取 HEAD、生成目标 index 字节并算 SHA-256。
///
/// 只读 staging，不改 live。staging 无 .git 或无 HEAD 时返回 None 字段。
fn build_staging_plan(staging_root: &Path) -> Result<StagingPlan> {
    let staging_git_dir = staging_root.join(".git");
    if !staging_git_dir.exists() {
        return Ok(StagingPlan {
            new_index_sha256: None,
            staging_head_oid: None,
            staging_branch_name: String::new(),
        });
    }

    let staging_repo = git2::Repository::open(staging_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "build_staging_plan: open staging repo: {e}"
        )))
    })?;

    let staging_head = staging_repo.head().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "build_staging_plan: staging HEAD missing: {e}"
        )))
    })?;
    let new_oid = staging_head.target().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(
            "build_staging_plan: staging HEAD is unborn (no target OID)",
        ))
    })?;

    // #644 评论 5483920624 问题4：shorthand 取不到 → Err，不 fallback main。
    let branch_name = staging_head
        .shorthand()
        .ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "build_staging_plan: staging HEAD shorthand() returned None — \
                 cannot determine branch name, refusing to guess main",
            ))
        })?
        .to_string();

    // #644 评论 5480360027：在 staging .git 目录下生成临时 index 文件，算 SHA-256。
    // 临时文件用 RAII 守卫保证清理。
    let tmp_id = uuid::Uuid::new_v4().to_string();
    let tmp_index_path = staging_git_dir.join(format!("index.sujian-tmp-{}", tmp_id));
    let tmp_index_guard = TmpFileGuard::new(tmp_index_path.clone());

    // 用 Index::open 打开临时路径（libgit2 会创建空 index 并关联 path）。
    let mut index = git2::Index::open(&tmp_index_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "build_staging_plan: open tmp index: {e}"
        )))
    })?;

    let new_commit = staging_repo.find_commit(new_oid).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "build_staging_plan: find commit: {e}"
        )))
    })?;
    let new_tree = new_commit.tree().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "build_staging_plan: find tree: {e}"
        )))
    })?;
    index.read_tree(&new_tree).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "build_staging_plan: read_tree: {e}"
        )))
    })?;
    index.write().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "build_staging_plan: write tmp index: {e}"
        )))
    })?;

    let index_bytes = fs::read(&tmp_index_path)?;
    let index_hash = sha256_bytes(&index_bytes);
    drop(tmp_index_guard);

    Ok(StagingPlan {
        new_index_sha256: Some(index_hash),
        staging_head_oid: Some(new_oid.to_string()),
        staging_branch_name: branch_name,
    })
}

/// #644 评论 5480360027：结合 snapshot + seed_state + staging_plan + staging_root
/// 构造完整 plan。
///
/// plan 的 old_oid 来自 snapshot（live 当前状态），new_oid 来自 staging_plan。
/// remote-tracking ref 的 new_oid 从 staging repo 读取。
#[allow(clippy::excessive_nesting)]
fn build_finalize_plan(
    seed_state: &GitSeedState,
    snapshot_refs: &std::collections::BTreeMap<String, RefSnapshot>,
    staging_plan: &StagingPlan,
    staging_root: &Path,
) -> Result<GitFinalizePlan> {
    let Some(ref new_oid_str) = staging_plan.staging_head_oid else {
        // staging 无 HEAD → finalize 不会做任何 Git metadata 变更，
        // 但 NotGitRepo 路径仍会创建 repo（copy .git from staging）。
        return Ok(GitFinalizePlan {
            repo_create: matches!(seed_state, GitSeedState::NotGitRepo),
            new_index_sha256: None,
            ref_plans: Vec::new(),
            repo_create_owner: new_repo_create_owner(seed_state),
            index_lock_owner: None,
        });
    };

    // #644 评论 5484539222 缺陷1：当本轮会写 index（new_index_sha256.is_some()）时，
    // 生成 index_lock_owner uuid，供 OwnedIndexLock 做持久 ownership。
    let index_lock_owner = if staging_plan.new_index_sha256.is_some() {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        None
    };

    let mut ref_plans: Vec<(String, Option<String>, String)> = Vec::new();

    match seed_state {
        GitSeedState::NotGitRepo => {
            // NotGitRepo 路径：创建 refs/heads/<branch>。
            // #644 评论 5484539222 缺陷3c：删掉 main fallback。
            // 按 build_staging_plan 的新不变量，只要有 staging_head_oid，
            // branch name 就必须已经解析成功；保留 main fallback 只会把真正的状态错误藏起来。
            if staging_plan.staging_branch_name.is_empty() {
                return Err(crate::Error::Io(std::io::Error::other(
                    "build_finalize_plan: NotGitRepo path requires non-empty \
                     staging_branch_name, refusing to fallback to \"main\"",
                )));
            }
            let branch_name = &staging_plan.staging_branch_name;
            let ref_name = format!("refs/heads/{}", branch_name);
            ref_plans.push((ref_name, None, new_oid_str.clone()));
        }
        GitSeedState::Unborn { head_ref } => {
            // Unborn 路径：创建 head_ref（old=None）。
            ref_plans.push((head_ref.clone(), None, new_oid_str.clone()));
        }
        GitSeedState::Existing { head_ref, head_oid } => {
            // Existing 路径：更新 head_ref（old=head_oid, new=staging HEAD）。
            ref_plans.push((
                head_ref.clone(),
                Some(head_oid.to_string()),
                new_oid_str.clone(),
            ));
        }
        GitSeedState::Detached { head_oid } => {
            // Detached 路径：更新 HEAD（old=head_oid, new=staging HEAD）。
            ref_plans.push((
                "HEAD".to_string(),
                Some(head_oid.to_string()),
                new_oid_str.clone(),
            ));
        }
    }

    // remote-tracking refs：从 snapshot 收集所有 refs/remotes/*，old_oid 来自 snapshot，
    // new_oid 从 staging repo 读取。staging 没有的 remote ref 不在 plan 中
    //（finalize 不会改它）。
    let staging_git_dir = staging_root.join(".git");
    if staging_git_dir.exists() {
        let staging_repo = git2::Repository::open(staging_root).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "build_finalize_plan: open staging repo: {e}"
            )))
        })?;
        let staging_refs = staging_repo.references().map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "build_finalize_plan: iterate staging references: {e}"
            )))
        })?;
        for reference in staging_refs {
            let reference = reference.map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "build_finalize_plan: staging reference iterator error: {e}"
                )))
            })?;
            let Some(name) = reference.name() else {
                continue;
            };
            if !name.starts_with("refs/remotes/") {
                continue;
            }
            let Some(target) = reference.target() else {
                continue;
            };
            let old_oid = match snapshot_refs.get(name) {
                Some(RefSnapshot::Existed { oid }) => Some(oid.clone()),
                _ => None,
            };
            ref_plans.push((name.to_string(), old_oid, target.to_string()));
        }
    }

    Ok(GitFinalizePlan {
        repo_create: matches!(seed_state, GitSeedState::NotGitRepo),
        new_index_sha256: staging_plan.new_index_sha256,
        ref_plans,
        repo_create_owner: new_repo_create_owner(seed_state),
        index_lock_owner,
    })
}

/// #644 评论 5481496190 问题3：NotGitRepo 路径生成 owner marker uuid。
/// 非 NotGitRepo 路径返回 None。
fn new_repo_create_owner(seed_state: &GitSeedState) -> Option<String> {
    if matches!(seed_state, GitSeedState::NotGitRepo) {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        None
    }
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
fn snapshot_ref(
    repo: &git2::Repository,
    ref_name: &str,
) -> std::result::Result<RefSnapshot, crate::Error> {
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

/// #644 评论 5482310913 问题3：Git finalize rollback 的明确结果。
///
/// `rollback_git_finalize` 不再返回 `Result<()>`，而是 `Result<GitRollbackOutcome>`，
/// 让上层 `rollback_full_sync_transaction` 能区分"已安全回滚，可继续恢复文件 backup"
/// 与"检测到并发变更或事务已成功，不能继续回滚文件"。
///
/// - `Reverted`：所有 index/ref 反向 CAS 都完整成功，上层可恢复文件 backup。
/// - `ConcurrentChanged`：repo_create ownership 不匹配（外部仓库）等无法证明归属的情况。
///   保留 transaction，不继续改 live 文件。上层应返回 Err 让 `recover_pending_transactions`
///   保留事务目录给下次恢复。
/// - `RepoInstallCommitted`：NotGitRepo 已完成 owner-matched `.git` rename。
///   按 commit-point 逻辑收尾，不再把文件回滚成旧版。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitRollbackOutcome {
    /// 所有 index/ref 反向 CAS 都完整成功，上层可恢复文件 backup。
    Reverted,
    /// repo_create ownership 不匹配 / 外部仓库，保留 transaction 不继续改 live 文件。
    ConcurrentChanged,
    /// NotGitRepo 已完成 owner-matched `.git` rename，按 commit-point 逻辑收尾。
    RepoInstallCommitted,
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
///
/// #644 评论 5480360027：接收 `plan`（write-ahead plan），不再维护内存 mutation_log。
/// finalize 成功就 finish，失败/崩溃恢复都只依赖磁盘上的 plan。
pub fn commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
) -> std::result::Result<(), GitFinalizeError> {
    // 调用内部 finalize，失败时按错误类型决定是否 rollback Git metadata。
    if let Err(e) =
        finalize_git_repo_metadata_inner(live_root, staging_root, seed_state, snapshot, plan)
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
                match rollback_git_finalize(live_root, snapshot, plan) {
                    Ok(GitRollbackOutcome::Reverted) => {
                        // Git metadata 已回滚，上层可继续回滚文件。
                        return Err(e);
                    }
                    Ok(GitRollbackOutcome::ConcurrentChanged) => {
                        // 检测到并发变更，保留 transaction 给下次恢复。
                        let rb_msg = "rollback detected concurrent change, transaction preserved"
                            .to_string();
                        log::warn!(
                            "commit_git_finalize: rollback saw concurrent change: {} \
                             (original error: {})",
                            rb_msg,
                            e
                        );
                        return Err(GitFinalizeError::RollbackFailed {
                            finalize: e.to_string(),
                            rollback: rb_msg,
                        });
                    }
                    Ok(GitRollbackOutcome::RepoInstallCommitted) => {
                        // marker 匹配说明 rename 已发生，状态与 finalize 失败矛盾，
                        // 保留 transaction 给下次恢复。
                        let rb_msg =
                            "rollback saw repo install committed, state inconsistent".to_string();
                        log::warn!(
                            "commit_git_finalize: rollback saw repo install committed: {} \
                             (original error: {})",
                            rb_msg,
                            e
                        );
                        return Err(GitFinalizeError::RollbackFailed {
                            finalize: e.to_string(),
                            rollback: rb_msg,
                        });
                    }
                    Err(rb_err) => {
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
                }
            }
        }
    }
    Ok(())
}

/// #644 评论 5485518160 修改点 2：`index_lock_owner=None` 旧 manifest 迁移判定结果。
///
/// 当 `plan.new_index_sha256.is_some()` 且 `plan.index_lock_owner.is_none()` 时，
/// 恢复入口（`rollback_full_sync_transaction`）在调用 `rollback_git_finalize` 之前
/// 用 `check_index_lock_owner_migration` 判断磁盘状态，决定如何处理：
pub enum IndexLockOwnerMigration {
    /// canonical `.git/index.lock` 已存在。不知道是谁的，不能 terminalize transaction，
    /// 返回 Err 保留 tx_dir。
    LockExists,
    /// 没有 `index.lock` 且 current index == snapshot.index（old）。index 这一步
    /// 本来就没发生，安全按 AlreadyReverted 继续（无需补 owner，直接让
    /// rollback_git_finalize 走 current==old 分支）。
    AlreadyReverted,
    /// 没有 `index.lock` 且 current index == new（plan.new_index_sha256 命中）。
    /// 生成新的 owner UUID，调用方应先原子写回 manifest.git_finalize.plan.index_lock_owner
    /// 并 fsync，再进入反向 rollback（此时 plan.index_lock_owner 已是 Some，
    /// rollback_git_finalize 的 current==new 分支能拿到 owner）。
    MigrateToNewOwner(String),
    /// current 既不是 old 也不是 new。并发修改，返回 Err 保留事务。
    ConcurrentModification,
}

impl std::fmt::Debug for IndexLockOwnerMigration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LockExists => write!(f, "LockExists"),
            Self::AlreadyReverted => write!(f, "AlreadyReverted"),
            Self::MigrateToNewOwner(_) => write!(f, "MigrateToNewOwner(<uuid>)"),
            Self::ConcurrentModification => write!(f, "ConcurrentModification"),
        }
    }
}

/// #644 评论 5485518160 修改点 2：检查 `index_lock_owner=None` 旧 manifest 的迁移判定。
///
/// 读 live `.git/index`、`.git/index.lock`，根据磁盘状态返回迁移决策。
/// 调用方（`rollback_full_sync_transaction`）负责持久化新 owner 到 manifest。
pub fn check_index_lock_owner_migration(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
) -> Result<IndexLockOwnerMigration> {
    let git_dir = live_root.join(".git");
    let lock_path = git_dir.join("index.lock");
    let index_path = git_dir.join("index");

    // canonical index.lock 已存在：不知道是谁的，不能 terminalize。
    if lock_path.exists() {
        return Ok(IndexLockOwnerMigration::LockExists);
    }

    // 没有 index.lock，读 current index。
    let current_index = if index_path.exists() {
        let bytes = fs::read(&index_path)?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };

    // 先判 current == old（snapshot.index）。
    if index_snapshot_eq(&current_index, &snapshot.index) {
        return Ok(IndexLockOwnerMigration::AlreadyReverted);
    }

    // 再判 current == new（plan.new_index_sha256）。
    if let Some(expected_hash) = plan.new_index_sha256 {
        let current_is_new = match &current_index {
            IndexSnapshot::Bytes(b) => sha256_bytes(b) == expected_hash,
            IndexSnapshot::Missing => false,
        };
        if current_is_new {
            // 生成新的 owner UUID，调用方负责持久化到 manifest。
            return Ok(IndexLockOwnerMigration::MigrateToNewOwner(
                uuid::Uuid::new_v4().to_string(),
            ));
        }
    }

    // current 既不是 old 也不是 new → 并发修改。
    Ok(IndexLockOwnerMigration::ConcurrentModification)
}

/// #644 评论 5480360027：CAS-based rollback，根据 plan 的 old_oid/new_oid 做反向 CAS。
///
/// 恢复时根据当前值判断某一步是否真的发生：
/// - current == new_oid → 这步已执行，反向 CAS 回 old_oid；
/// - current == old_oid → 这步没执行，无需 rollback；
/// - 两者都不是 → 有并发新状态，不能覆盖。
///
/// #644 评论 5480360027 修复点 4：更新型 ref rollback 用 `reference_matching`
/// 反向 CAS，不再用 `force=true` 的 `reference()`。
///
/// #644 评论 5482310913 问题2/3：
/// - 问题2：`plan.repo_create=true` 时，**第一件事**先判 live `.git` ownership，
///   marker 不匹配 → 返回 `ConcurrentChanged`，不碰 index/lock/refs。
/// - 问题3：返回 `Result<GitRollbackOutcome>`。index/ref CAS miss（真正并发新状态）
///   返回 `Err`，让上层 `rollback_full_sync_transaction` 保留 transaction 不回滚文件。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub fn rollback_git_finalize(
    live_root: &Path,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
) -> Result<GitRollbackOutcome> {
    crate::storage::git_runtime::ensure_initialized()?;

    // 1. #644 评论 5482310913 问题2：repo_create=true 时先判 ownership，再碰 index/refs。
    //    外部仓库的 index/lock/refs 一字节都不能碰。
    if plan.repo_create {
        let live_git = live_root.join(".git");
        if !live_git.exists() {
            // 本轮 repo install 没发生（rename 前崩溃）。清理本轮对应的 tmp_git
            //（基于 repo_create_owner 命名，无需扫猜）。然后回滚文件。
            if let Some(owner) = &plan.repo_create_owner {
                let tmp_git = live_root.join(format!(".git.sujian-tmp-{}", owner));
                if tmp_git.exists() {
                    let _ = fs::remove_dir_all(&tmp_git);
                }
            }
            return Ok(GitRollbackOutcome::Reverted);
        }
        let marker_path = live_git.join(".sujian-sync-owner");
        let marker_matches = match (&plan.repo_create_owner, marker_path.exists()) {
            (Some(expected), true) => match fs::read_to_string(&marker_path) {
                Ok(content) => content == *expected,
                Err(_) => false,
            },
            _ => false,
        };
        if !marker_matches {
            // #644 评论 5482310913 问题2：marker 不匹配 → 外部创建的仓库。
            // 不碰 index/lock/refs，不继续回滚 live 文件，保留 transaction 给下次恢复。
            log::warn!(
                "rollback_git_finalize: repo_create=true but live .git owner marker \
                 missing or mismatched — treating as externally created, NOT touching \
                 index/lock/refs"
            );
            return Ok(GitRollbackOutcome::ConcurrentChanged);
        }
        // marker 匹配 → 本轮创建的 .git，删除它回到 finalize 前状态。
        fs::remove_dir_all(&live_git)?;
        // #644 评论 5485518160 修改点 3：remove_dir_all 后必须 fsync live_root
        //（.git 的父目录），持久化 .git 目录项删除，之后才能返回 Reverted。
        crate::storage::sync_parent(&live_git)?;
        return Ok(GitRollbackOutcome::Reverted);
    }

    // 2. repo_create=false：index + refs rollback。
    //    #644 评论 5483239422 问题2/3 + #644 评论 5484539222 缺陷1：
    //    三态幂等 + lockfile 反向提交边界 + 持久 ownership。
    //    - current == old(snapshot.index) → AlreadyReverted / no-op，但需清理本轮 stale lock
    //    - current == new(plan.new_index_sha256) → 反向恢复（lockfile 反向提交）
    //    - 其它 → ConcurrentChanged (Err)
    //    反向恢复用 OwnedIndexLock（hard_link），lock 已存在且不属于本轮 → Err，
    //    绝不删别人的 lock（与 forward install_index_with_lock 同语义）。
    if let Some(expected_hash) = plan.new_index_sha256 {
        let index_path = live_root.join(".git").join("index");
        let lock_path = live_root.join(".git").join("index.lock");

        // 读取当前 index 字节（不存在视为 Missing）。
        let current_index = if index_path.exists() {
            let bytes = fs::read(&index_path)?;
            IndexSnapshot::Bytes(bytes)
        } else {
            IndexSnapshot::Missing
        };

        // 三态判断：先判 old（AlreadyReverted），再判 new（需反向恢复），其它并发失败。
        if index_snapshot_eq(&current_index, &snapshot.index) {
            // current == old(snapshot.index) → 第一次 rollback 已恢复，no-op。
            // #644 评论 5486167472 问题1 + #644 评论 5486852142 问题1+问题2：
            // 目录锁模型下检测并清理本轮的 stale lock。
            // forward install 中途被 SIGKILL（acquire 后、commit_rename 前）会残留
            // .git/index.lock 目录（含 owner + prepared 文件）。
            // - lock_path 是目录且归属本轮（owner 匹配或 owner 文件不存在/为空）
            //   → 清理 lock 目录，继续 no-op。
            // - lock_path 是 regular file（外部 Git 进程的 lock）
            //   → #644 评论 5486852142 问题2：返回 ConcurrentChanged，保留 transaction。
            // - lock_path 是目录但 owner 不匹配（不同事务的 lock）
            //   → #644 评论 5486852142 问题2：返回 ConcurrentChanged，保留 transaction。
            // #644 评论 5485518160 修改点 2：迁移入口已保证进入 rollback_git_finalize 时
            // 若 new_index_sha256.is_some() 则 owner 必为 Some（除非 current==old 且无
            // lock 的安全 no-op 路径）。owner=None 且有 stale lock → 未知归属，返回 Err
            // 保留事务（不能留永久 lock）。
            if let Some(owner) = &plan.index_lock_owner {
                match lock_dir_belongs_to_owner(&lock_path, owner) {
                    LockOwner::Ours => {
                        // lock 目录属于本轮：清理 lock 目录，fsync .git。
                        if remove_lock_dir_if_exists(&lock_path, &live_root.join(".git")) {
                            // remove_lock_dir_if_exists 已 sync_dir。
                        }
                    }
                    LockOwner::External => {
                        // #644 评论 5486852142 问题2：lock 不属于本轮
                        //（regular file 外部 Git lock 或目录锁但 owner 不匹配）。
                        // 返回 ConcurrentChanged，保留 transaction 不继续 rollback refs。
                        return Ok(GitRollbackOutcome::ConcurrentChanged);
                    }
                    LockOwner::Absent => {
                        // lock 不存在：no-op，无需清理。
                    }
                }
            } else if lock_path.exists() {
                // #644 评论 5485518160 修改点 2：owner=None 但 stale lock 存在。
                // 迁移入口应已处理 owner=None 情况，到达这里说明 lock 是未知的
                //（可能外部 Git 进程），不能留永久 lock 也不能删别人的 lock，
                // 返回 Err 保留事务给下次恢复。
                return Err(crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: current == snapshot.index but \
                     plan.index_lock_owner is None and stale index.lock exists — \
                     cannot determine lock ownership, refusing to delete external \
                     lock or leave permanent lock, preserving transaction for next \
                     recovery",
                )));
            }
        } else {
            // current != old，检查是否 == new（plan.new_index_sha256）。
            let current_is_new = match &current_index {
                IndexSnapshot::Bytes(b) => sha256_bytes(b) == expected_hash,
                IndexSnapshot::Missing => false,
            };
            if !current_is_new {
                // current 既不是 old 也不是 new → 真正并发修改。
                return Err(crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: index CAS miss (concurrent modification, \
                     current matches neither snapshot.index nor plan.new_index_sha256) — \
                     refusing to continue rollback to preserve transaction for next recovery",
                )));
            }
            // current == new(plan) → 需要反向恢复到 snapshot.index。
            // 走 OwnedIndexLock 反向提交边界（与 forward install_indexE_with_lock 同语义）：
            // 1. acquire（create_dir + owner metadata）自己获取 .git/index.lock 目录；
            //    lock 已存在且不属于本轮 → Err，绝不删。
            // 2. 拿到锁后重新读 index 确认仍 == new。
            // 3. Bytes 路径 commit_rename（rename prepared_file → index）；Missing 路径 commit_delete。
            // #644 评论 5486167472 问题1：用 plan.index_lock_owner 做持久 ownership。
            let owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
                crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: plan.index_lock_owner is None but \
                     new_index_sha256 is Some — cannot acquire OwnedIndexLock without owner \
                     (plan was generated by older code without persistent index lock ownership)",
                ))
            })?;
            rollback_index_via_lockfile(
                &index_path,
                &lock_path,
                expected_hash,
                &snapshot.index,
                owner,
            )?;
        }
    }

    // 3. Rollback refs — CAS-based: only restore if current value == what we wrote.
    if plan.ref_plans.is_empty() {
        return Ok(GitRollbackOutcome::Reverted);
    }

    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "rollback_git_finalize: open live repo: {e}"
        )))
    })?;

    for (ref_name, old_oid_str, new_oid_str) in &plan.ref_plans {
        let new_oid = git2::Oid::from_str(new_oid_str).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "rollback_git_finalize: invalid new_oid for {}: {e}",
                ref_name
            )))
        })?;

        // Check current value.
        let current = live_repo.find_reference(ref_name);

        match (&old_oid_str, current) {
            // ref(old=None): finalize 会新建 ref。
            // - current absent → no-op（AlreadyReverted 或从未创建）
            // - current == new → delete（反向）
            // - current == 其它 → ConcurrentChanged (Err)
            (None, Ok(current_ref)) => {
                if current_ref.target() == Some(new_oid) {
                    // Still our value — safe to delete.
                    let mut r = current_ref;
                    r.delete().map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rollback_git_finalize: delete {}: {e}",
                            ref_name
                        )))
                    })?;
                } else {
                    // #644 评论 5482310913 问题3 + #644 评论 5483239422 问题2：
                    // ref CAS miss → 真正并发新状态。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: ref {} CAS miss (current != new_oid {}) — \
                         concurrent modification, refusing to continue rollback to preserve \
                         transaction",
                        ref_name, new_oid
                    ))));
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

            // ref(old=Some): finalize 会更新 ref old_oid -> new_oid。
            // #644 评论 5483239422 问题2：三态幂等。
            // - current == old → no-op（第一次 rollback 已反向 CAS 回 old，或本轮 finalize 未执行）
            // - current == new → 反向 CAS 回 old（reference_matching expected=new, write=old）
            // - current == 其它 / NotFound → ConcurrentChanged (Err)
            (Some(old_oid_str), Ok(current_ref)) => {
                let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: invalid old_oid for {}: {e}",
                        ref_name
                    )))
                })?;
                if current_ref.target() == Some(old_oid) {
                    // current == old → no-op（AlreadyReverted 或本轮 finalize 未执行）。
                } else if current_ref.target() == Some(new_oid) {
                    // current == new → 反向 CAS：只有 current == new_oid 时才写 old_oid。
                    live_repo
                        .reference_matching(
                            ref_name,
                            old_oid,
                            true,
                            new_oid,
                            "rollback: CAS restore ref",
                        )
                        .map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "rollback_git_finalize: CAS restore {} failed (expected current={} new={}): {}",
                                ref_name, new_oid, old_oid, e
                            )))
                        })?;
                } else {
                    // current 既不是 old 也不是 new → 真正并发新状态。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: ref {} CAS miss (current matches neither \
                         old_oid {} nor new_oid {}) — concurrent modification, refusing to \
                         continue rollback to preserve transaction",
                        ref_name, old_oid, new_oid
                    ))));
                }
            }
            (Some(old_oid_str), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // #644 评论 5483239422 问题2：ref(old=Some) 但当前 NotFound。
                // old 存在过，finalize 应把它从 old 改到 new，现在 ref 消失了，
                // 说明被并发进程删除——不能重建（重建会覆盖并发的删除意图）。
                // 返回 Err 让上层保留 transaction。
                let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: invalid old_oid for {}: {e}",
                        ref_name
                    )))
                })?;
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "rollback_git_finalize: ref {} was deleted by concurrent process \
                     (expected old_oid {} or new_oid {}) — refusing to recreate to preserve \
                     concurrent deletion, transaction retained for next recovery",
                    ref_name, old_oid, new_oid
                ))));
            }
            (Some(_), Err(e)) => {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "rollback_git_finalize: lookup {}: {e}",
                    ref_name
                ))));
            }
        }
    }

    Ok(GitRollbackOutcome::Reverted)
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
///
/// #644 评论 5480360027：接收 `plan`，不再维护内存 mutation_log。
pub fn recover_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
) -> std::result::Result<(), GitFinalizeError> {
    // 尝试完成 Git finalize。
    match finalize_git_repo_metadata_inner(live_root, staging_root, seed_state, snapshot, plan) {
        Ok(()) => {
            log::info!("recover_git_finalize: successfully completed pending Git finalize");
            Ok(())
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
                match rollback_git_finalize(live_root, snapshot, plan) {
                    Ok(GitRollbackOutcome::Reverted) => {
                        // Git metadata 已回滚，上层可继续回滚文件。
                        Err(e)
                    }
                    Ok(GitRollbackOutcome::ConcurrentChanged) => {
                        // 检测到并发变更，保留 transaction 给下次恢复。
                        Err(GitFinalizeError::ConcurrentMetadataChanged {
                            reason: format!(
                                "rollback detected concurrent change during recovery (original finalize error: {})",
                                e
                            ),
                        })
                    }
                    Ok(GitRollbackOutcome::RepoInstallCommitted) => {
                        // #644 评论 5482310913 问题2：marker 匹配说明 rename 已发生，
                        // .git 已完整安装，事务已成功，按 commit-point 逻辑收尾。
                        log::info!(
                            "recover_git_finalize: rollback saw repo install committed \
                             (owner-matched .git rename completed), treating tx as successful"
                        );
                        Ok(())
                    }
                    Err(rb_err) => Err(GitFinalizeError::RollbackFailed {
                        finalize: e.to_string(),
                        rollback: rb_err.to_string(),
                    }),
                }
            }
        },
    }
}

/// 包装函数，供 `sync_ops.rs` 调用。
///
/// #644 评论 5476546134 第4节：返回 `GitFinalizeError`，上层遇到 `RollbackFailed`
/// 时必须保留 transaction 目录。
///
/// #644 评论 5480360027：接收 `plan`，不再返回 mutation_log。
pub fn try_commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: Option<&GitSeedState>,
    snapshot: Option<&GitMetadataSnapshot>,
    plan: Option<&GitFinalizePlan>,
) -> std::result::Result<(), GitFinalizeError> {
    let Some(state) = seed_state else {
        return Ok(());
    };
    let Some(snap) = snapshot else {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other("missing GitMetadataSnapshot for Git backend"),
        )));
    };
    let Some(plan) = plan else {
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other("missing GitFinalizePlan for Git backend"),
        )));
    };
    commit_git_finalize(live_root, staging_root, state, snap, plan)
}

/// #644 评论 5482310913 问题1：成功收尾时清理 owner marker。
///
/// `finalize_not_git_repo` rename 成功后**不再立即删除** `.sujian-sync-owner`，
/// 而是推迟到上层 `tx.finish()` 之后调用本函数。这样：
/// - crash 在 `tx.finish()` 前：marker 还在，恢复知道这个 repo 是本轮创建的。
/// - crash 在 `tx.finish()` 后、marker 删除前：事务已成功，只会留下一个无害 marker，
///   下次顺手清理即可。
///
/// `plan.repo_create_owner` 为 `None` 时是 no-op（非 NotGitRepo 路径）。
/// marker 不存在时也是 no-op（已清理或从未创建）。
pub fn cleanup_repo_create_owner_marker(live_root: &Path, plan: &GitFinalizePlan) {
    if plan.repo_create_owner.is_some() {
        let marker_path = live_root.join(".git").join(".sujian-sync-owner");
        let _ = fs::remove_file(&marker_path);
    }
}

// ── 内部 finalize 实现 ──

/// 内部 finalize 实现，不含 rollback（由调用方处理）。
///
/// #644 评论 5477439446 问题2：返回 `GitFinalizeError`，使 `ConcurrentMetadataChanged`
/// 能向上传播到 `commit_git_finalize`，由其决定是否 rollback。
/// 接收 `snapshot` 用于 finalize_unborn/finalize_existing 的并发校验。
///
/// #644 评论 5480360027：接收 `plan`（write-ahead plan），不再维护内存 mutation_log。
#[allow(clippy::too_many_lines)]
fn finalize_git_repo_metadata_inner(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
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

    // #644 评论 5483920624 问题4：shorthand 取不到 → Err，不 fallback main。
    let branch_name = staging_head
        .shorthand()
        .ok_or_else(|| {
            crate::Error::Io(std::io::Error::other(
                "finalize: staging HEAD shorthand() returned None — \
                 cannot determine branch name, refusing to guess main",
            ))
        })?
        .to_string();

    match seed_state {
        GitSeedState::NotGitRepo => finalize_not_git_repo(
            live_root,
            staging_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            &branch_name,
            plan,
        ),
        GitSeedState::Unborn { head_ref } => finalize_unborn(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            head_ref,
            snapshot,
            plan,
        ),
        GitSeedState::Existing { head_ref, head_oid } => finalize_existing(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            head_ref,
            *head_oid,
            snapshot,
            plan,
        ),
        GitSeedState::Detached { head_oid } => finalize_detached(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            *head_oid,
            snapshot,
            plan,
        ),
    }
}

/// finalize 路径 1：live 原本不是 Git repo。
///
/// #644 评论 5475413230 第2节：原子性改进。
/// #644 评论 5475805198 第4节：RAII 守卫保证临时目录清理。
/// #644 评论 5478237852 问题1：返回 `GitFinalizeError` 而非 `crate::Error`，
/// 使 `ConcurrentMetadataChanged` 能向上传播，避免 rollback 删除别人刚创建的 `.git`。
///
/// #644 评论 5480360027：不再接收 mutation_log。plan 已在 prepare 阶段落盘。
fn finalize_not_git_repo(
    live_root: &Path,
    staging_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    branch_name: &str,
    plan: &GitFinalizePlan,
) -> std::result::Result<(), GitFinalizeError> {
    let staging_git = staging_root.join(".git");
    let live_git = live_root.join(".git");

    // #644 评论 5482310913 问题2：tmp_git 目录名基于 repo_create_owner，
    // 使恢复时能精准清理本轮 tmp repo，不用扫猜。
    // owner 为 None 时（旧 plan 兼容）回退到随机 uuid。
    let tmp_id = plan
        .repo_create_owner
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
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

    // NotGitRepo 路径：index 在 tmp repo 中写入，rename 后整体安装到 live。
    // 不需要 index lock（live 还没有 .git）。
    update_live_index(&tmp_repo, staging_repo, new_oid)?;

    // #644 评论 5481496190 问题3 + #644 评论 5482310913 问题1：
    // 在 rename 前写入 owner marker 到 tmp_git。
    // #644 评论 5482310913 问题1：marker 写入用 atomic_write_bytes（含 fsync 文件 +
    // fsync 父目录），保证 marker 是 durable ownership fact，不能只 fs::write。
    // rename 后 marker 跟着进入 live .git。rollback 只有 marker 匹配才删除 live .git。
    // marker 文件名 .sujian-sync-owner，内容是 plan.repo_create_owner（uuid）。
    if let Some(owner) = &plan.repo_create_owner {
        let marker_path = _guard.path().join(".sujian-sync-owner");
        crate::storage::atomic_write_bytes(&marker_path, owner.as_bytes())
            .map_err(GitFinalizeError::FinalizeFailed)?;
    }

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
            std::io::Error::other(format!(
                "finalize_not_git_repo: rename tmp -> .git failed: {e}"
            )),
        )));
    }

    // #644 评论 5485518160 修改点 3：rename 成功后必须 fsync live_root（.git 的父目录），
    // 持久化 .git 目录项，之后才能返回 finalize success。
    crate::storage::sync_parent(&live_git).map_err(GitFinalizeError::FinalizeFailed)?;

    // #644 评论 5482310913 问题1：rename 成功后**不要删除** owner marker。
    // marker 删除推迟到上层 `tx.finish()` 之后（由 `cleanup_repo_create_owner_marker` 负责）。
    // 崩溃窗口分析：
    // - crash 在 tx.finish() 前：marker 还在，恢复知道这个 repo 是本轮创建的。
    // - crash 在 tx.finish() 后、marker 删除前：事务已成功，只会留下一个无害 marker，
    //   下次顺手清理即可。
    Ok(())
}

/// finalize 路径 2：live 是 unborn repo。
///
/// #644 评论 5475805198 第3节：使用 `find_reference("HEAD")` 读取未 resolve 的 HEAD，
/// 确认 `symbolic_target() == seed head_ref` 且目标 branch ref 仍不存在。
///
/// #644 评论 5477439446 问题2：在第一次改 live Git metadata 之前做并发校验，
/// 校验失败返回 `ConcurrentMetadataChanged`（不触发 rollback）。
///
/// #644 评论 5480360027：接收 `plan`，不再维护 mutation_log。
/// index 写入改为：先在临时目录生成目标 index，获取 `.git/index.lock`，
/// 拿到锁后重新读取 live index 确认仍等于 snapshot，一致才原子安装。
fn finalize_unborn(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
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

    // #644 评论 5480360027 修复点 3 + #644 评论 5484539222 缺陷1：index 原生锁边界 + 持久 ownership。
    // 先在临时目录生成目标 index，获取 .git/index.lock（OwnedIndexLock hard_link），
    // 拿到锁后重新读取 live index 确认仍等于 snapshot，一致才原子安装。
    let index_lock_owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
            "finalize_unborn: plan.index_lock_owner is None but new_index_sha256 is Some",
        )))
    })?;
    install_index_with_lock(
        live_root,
        &live_repo,
        staging_repo,
        new_oid,
        snapshot,
        index_lock_owner,
    )?;

    // #644 评论 5480360027 修复点 5：sync_remote_refs 严格错误传播。
    sync_remote_refs(&live_repo, staging_repo, &snapshot.refs)?;

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
    // #644 评论 5484539222 缺陷3b：严格 match find_reference 返回。
    // - Ok(_)：branch 已存在（concurrent modification），按 "already exists" 逻辑处理。
    // - Err(NotFound)：只有这里才表示 ref 不存在，继续创建。
    // - 其它 Err：直接返回 finalize failure，不继续。
    match live_repo.find_reference(head_ref) {
        Ok(_) => {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_unborn: branch ref {} already exists (concurrent modification)",
                    head_ref
                )),
            )));
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // branch 不存在，继续创建。
        }
        Err(e) => {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_unborn: find_reference({}) failed with non-NotFound error: {} \
                     — refusing to create ref on possibly corrupted refdb",
                    head_ref, e
                )),
            )));
        }
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
///
/// #644 评论 5477439446 问题2：在第一次改 live Git metadata 之前做并发校验，
/// 校验失败返回 `ConcurrentMetadataChanged`（不触发 rollback）。
///
/// #644 评论 5480360027：接收 `plan`，不再维护 mutation_log。
#[allow(clippy::too_many_arguments)]
fn finalize_existing(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    base_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
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

    // #644 评论 5480360027 修复点 3 + #644 评论 5484539222 缺陷1：index 原生锁边界 + 持久 ownership。
    let index_lock_owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
            "finalize_existing: plan.index_lock_owner is None but new_index_sha256 is Some",
        )))
    })?;
    install_index_with_lock(
        live_root,
        &live_repo,
        staging_repo,
        new_oid,
        snapshot,
        index_lock_owner,
    )?;

    // #644 评论 5480360027 修复点 5：sync_remote_refs 严格错误传播。
    sync_remote_refs(&live_repo, staging_repo, &snapshot.refs)?;

    // #644 评论 5475805198 第3节 + #644 评论 5484539222 缺陷3a：确认 HEAD 仍指向 seed 时同一个 branch。
    // 防止用户切换到别的 branch 后还偷偷更新旧 branch。
    // 严格 match find_reference("HEAD") 返回：
    // - Ok(raw_head)：按当前真实状态处理（校验 sym_target == head_ref）。
    // - Err(NotFound)：HEAD 不存在（unborn 或被并发删），返回 finalize failure，
    //   绝不静默跳过（旧代码 if let Ok 把 IO/refdb 损坏错误跳过）。
    // - 其它 Err：直接返回 finalize failure，进入已经存在的 rollback 路径。
    match live_repo.find_reference("HEAD") {
        Ok(raw_head) => {
            if let Some(sym_target) = raw_head.symbolic_target() {
                if sym_target != head_ref {
                    return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                        std::io::Error::other(format!(
                            "finalize_existing: HEAD now points to {} but seed was {}",
                            sym_target, head_ref
                        )),
                    )));
                }
                // HEAD 仍是 symbolic 且 target == head_ref：允许继续。
            } else {
                // #644 评论 5485518160 修改点 4：HEAD detached（无 sym_target）必须返回
                // FinalizeFailed（**不是** ConcurrentMetadataChanged，因为本轮已经可能
                // 写过 index/remote refs，需要走现有 rollback，不能标成"nothing written"）。
                // verify_git_metadata_unchanged 是在 index/remote refs 写入之前做的，
                // 用户/外部 Git 可在 preflight 后把 HEAD detach。此时 index 已换、remote
                // refs 已改，post-check 看到 detached 也接受的话，只要旧 branch 仍是
                // base_oid，reference_matching(head_ref, ...) 仍会成功，full sync 偷偷
                // 推进用户已离开的 branch。收紧为 detached → FinalizeFailed，理由说明
                // "HEAD detached after preflight: index/refs may already be written,
                // must rollback"。
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(
                        "finalize_existing: HEAD detached after preflight: index/refs \
                         may already be written, must rollback (refusing to advance \
                         head_ref on a branch the user has left via detach)",
                    ),
                )));
            }
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_existing: HEAD reference not found (unborn or deleted): {} \
                     — refusing to finalize on repo without HEAD",
                    e
                )),
            )));
        }
        Err(e) => {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_existing: find_reference(\"HEAD\") failed with non-NotFound \
                     error: {} — refusing to finalize on possibly corrupted refdb",
                    e
                )),
            )));
        }
    }

    // #644 评论 5481496190 问题1：force=true 让 libgit2 能更新已存在的 ref。
    // current_id=base_oid 提供 CAS 保护，current != base_oid 时返回 EMODIFIED，
    // 只有当前值 == base_oid 才写 new_oid。force=false 对已存在 ref 直接返回
    // GIT_EEXISTS，current_id 的 CAS 检查不会被执行。
    live_repo
        .reference_matching(
            head_ref,
            new_oid,
            true,
            base_oid,
            "sync: finalize git repo metadata after full sync",
        )
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: update-ref {} failed (CAS old={} new={}): {}",
                head_ref, base_oid, new_oid, e
            )))
        })?;

    // #644 评论 5486852142 问题3：branch CAS 成功后再次校验 HEAD。
    // HEAD post-check（上方 match live_repo.find_reference("HEAD")）到 branch CAS 之间
    // 有窗口，用户可在此窗口切 branch（HEAD 仍 symbolic 指向别的 branch），目标 branch
    // 仍是 base_oid，branch CAS 成功，full sync 推进用户已离开的 branch。
    // branch CAS 成功后再次读取 HEAD：
    // - HEAD 仍 symbolic 且 target == head_ref → 成功。
    // - HEAD symbolic 但 target != head_ref / HEAD detached / HEAD NotFound / 其它 Err
    //   → 反向 CAS head_ref: new_oid -> base_oid，返回 FinalizeFailed
    //   （让现有 index/remote rollback 收尾）。
    verify_head_after_branch_cas(&live_repo, head_ref, new_oid, base_oid)?;

    Ok(())
}

/// #644 评论 5486852142 问题3：finalize_existing branch CAS 成功后再次校验 HEAD。
///
/// branch CAS 成功后再次读取 HEAD：
/// - HEAD 仍 symbolic 且 target == head_ref → Ok(())。
/// - HEAD symbolic 但 target != head_ref / HEAD detached / HEAD NotFound / 其它 Err
///   → 反向 CAS head_ref: new_oid -> base_oid，返回 FinalizeFailed
///   （让现有 index/remote rollback 收尾）。
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
fn verify_head_after_branch_cas(
    live_repo: &git2::Repository,
    head_ref: &str,
    new_oid: git2::Oid,
    base_oid: git2::Oid,
) -> std::result::Result<(), GitFinalizeError> {
    match live_repo.find_reference("HEAD") {
        Ok(post_head) => {
            if let Some(post_sym_target) = post_head.symbolic_target() {
                if post_sym_target != head_ref {
                    // HEAD 在 branch CAS 后切到了别的 branch。
                    // 反向 CAS head_ref: new_oid -> base_oid，撤销本轮 branch 推进。
                    live_repo
                        .reference_matching(
                            head_ref,
                            base_oid,
                            true,
                            new_oid,
                            "sync: rollback head_ref after HEAD changed post branch CAS",
                        )
                        .map_err(|e| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "finalize_existing: post-CAS HEAD switched to {} (expected {}), \
                                 reverse CAS head_ref {} failed (new={} old={}): {}",
                                post_sym_target, head_ref, head_ref, new_oid, base_oid, e
                            )))
                        })?;
                    return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                        std::io::Error::other(format!(
                            "finalize_existing: HEAD switched from {} to {} after branch CAS \
                             succeeded — reversed branch CAS ({}: {} -> {}), must rollback \
                             (refusing to advance head_ref on a branch the user has left)",
                            head_ref, post_sym_target, head_ref, new_oid, base_oid
                        )),
                    )));
                }
                // HEAD 仍 symbolic 且 target == head_ref → 成功。
            } else {
                // HEAD detached after branch CAS。反向 CAS 撤销本轮 branch 推进。
                live_repo
                    .reference_matching(
                        head_ref,
                        base_oid,
                        true,
                        new_oid,
                        "sync: rollback head_ref after HEAD detached post branch CAS",
                    )
                    .map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "finalize_existing: post-CAS HEAD detached, reverse CAS head_ref \
                             {} failed (new={} old={}): {}",
                            head_ref, new_oid, base_oid, e
                        )))
                    })?;
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(
                        "finalize_existing: HEAD detached after branch CAS succeeded — \
                         reversed branch CAS, must rollback (refusing to advance head_ref \
                         on a branch the user has left via detach)",
                    ),
                )));
            }
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // HEAD NotFound after branch CAS。反向 CAS 撤销本轮 branch 推进。
            live_repo
                .reference_matching(
                    head_ref,
                    base_oid,
                    true,
                    new_oid,
                    "sync: rollback head_ref after HEAD NotFound post branch CAS",
                )
                .map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "finalize_existing: post-CAS HEAD NotFound, reverse CAS head_ref \
                         {} failed (new={} old={}): {}",
                        head_ref, new_oid, base_oid, e
                    )))
                })?;
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_existing: HEAD reference not found after branch CAS succeeded \
                     (unborn or deleted): {} — reversed branch CAS, refusing to finalize on \
                     repo without HEAD",
                    e
                )),
            )));
        }
        Err(e) => {
            // 其它 Err：尝试反向 CAS，返回 FinalizeFailed。
            live_repo
                .reference_matching(
                    head_ref,
                    base_oid,
                    true,
                    new_oid,
                    "sync: rollback head_ref after HEAD read error post branch CAS",
                )
                .map_err(|e2| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "finalize_existing: post-CAS find_reference(\"HEAD\") failed: {}, \
                         reverse CAS head_ref {} also failed (new={} old={}): {}",
                        e, head_ref, new_oid, base_oid, e2
                    )))
                })?;
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_existing: find_reference(\"HEAD\") failed with non-NotFound \
                     error after branch CAS succeeded: {} — reversed branch CAS, refusing \
                     to finalize on possibly corrupted refdb",
                    e
                )),
            )));
        }
    }
    Ok(())
}

/// finalize 路径 4：live 是 detached HEAD。
///
/// #644 评论 5475805198 第3节：使用 `reference_matching("HEAD", ...)` 做真正的 CAS。
///
/// #644 评论 5481496190 问题4：返回 `GitFinalizeError` 而非 `crate::Error`，
/// 使 `ConcurrentMetadataChanged`（来自 verify_git_metadata_unchanged 或
/// install_index_with_lock）能原样向上传播到 `commit_git_finalize`，
/// 由其决定不 rollback。旧代码把 ConcurrentMetadataChanged 降级成 Io → FinalizeFailed，
/// 触发不该发生的 rollback，撤销并发方的 detached HEAD 更新。
///
/// #644 评论 5481496190 问题1：reference_matching 用 force=true 让 libgit2 能更新
/// 已存在的 HEAD，current_id=base_oid 提供 CAS 保护。
fn finalize_detached(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    base_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
) -> std::result::Result<(), GitFinalizeError> {
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
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
            std::io::Error::other(format!(
                "finalize_detached: new_oid {} not found in live after import",
                new_oid
            )),
        )));
    }

    // #644 评论 5478237852 问题2：preflight verify before writing.
    // #644 评论 5481496190 问题4：ConcurrentMetadataChanged 原样向上传播，不降级。
    verify_git_metadata_unchanged(live_root, snapshot, "HEAD")?;

    // #644 评论 5480360027 修复点 3 + #644 评论 5484539222 缺陷1：index 原生锁边界 + 持久 ownership。
    // #644 评论 5481496190 问题4：ConcurrentMetadataChanged 原样向上传播，不降级。
    let index_lock_owner = plan.index_lock_owner.as_deref().ok_or_else(|| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
            "finalize_detached: plan.index_lock_owner is None but new_index_sha256 is Some",
        )))
    })?;
    install_index_with_lock(
        live_root,
        &live_repo,
        staging_repo,
        new_oid,
        snapshot,
        index_lock_owner,
    )?;

    // #644 评论 5475805198 第3节：使用 reference_matching 做真正的 CAS。
    // #644 评论 5481496190 问题1：force=true 让 libgit2 能更新已存在的 HEAD，
    // current_id=base_oid 提供 CAS 保护，current != base_oid 时返回 EMODIFIED。
    live_repo
        .reference_matching(
            "HEAD",
            new_oid,
            true,
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

/// #644 评论 5486167472 问题1+问题2 + #644 评论 5486852142 问题1：持久 ownership 的 index lock。
///
/// ## IndexLockProtocol（目录锁模型，Android shared-storage 可落地）
///
/// Android 共享存储（AOSP FUSE）不提供 hardlink，`rename` 的带 flag 版本也不是
/// 可依赖的 no-replace CAS。本协议只用 Android shared-storage 能提供的原语：
/// `create_dir`（原子创建目录，已存在则失败）、`rename`（覆盖式）、`fsync`、读写文件内容。
///
/// - **canonical lock** = `.git/index.lock` 作为**目录**（不是 regular file）。
///   `fs::create_dir(&lock_path)` 原子创建目录，已存在则失败。标准 Git 创建 regular
///   `index.lock` 会因路径已存在（是目录）而失败，目录类型本身就是 Sujian 协议事实。
///   `create_dir` 成功就是原子的 ownership 证明，**不再有** O_EXCL create-to-write 窗口。
/// - 目录内放：
///   - `owner` 文件：写 owner metadata（`INDEX_LOCK_MARKER` + `owner=<owner>`），fsync。
///   - `prepared` 文件：写 prepared bytes（目标 index 内容），fsync。
/// - **ownership** 通过目录存在性 + 目录内 `owner` 文件内容证明。恢复时：
///   - `lock_path` 是 regular file → 外部 Git 进程的 lock（标准 Git 创建 regular file）。
///   - `lock_path` 是目录 → Sujian lock（不可能是外部 Git 的）。读 `owner` 文件判断归属：
///     - `owner` 文件不存在/为空 → Sujian lock 但未完成写入（crash 在 create_dir 和 write
///       owner 之间），可安全清理。
///     - `owner` 文件存在且内容匹配 → 本轮 lock。
///     - `owner` 文件存在但不匹配 → 不同事务的 lock。
/// - **commit_rename**：rename `prepared` → index（覆盖），然后删除 lock 目录，每步 fsync 父目录。
/// - **commit_delete**（Missing 路径）：删 index → 删 prepared → 删 lock 目录，每步 fsync 父目录。
///
/// 关键不变量：`create_dir` 成功就是原子的 ownership 证明。即使 crash 在 create_dir 后、
/// write owner 前，恢复时看到目录就知道是 Sujian lock（不可能是外部 Git 的），可以安全清理。
/// 不再有"空/半截 lock 被当成外部 Git lock"的窗口。
///
/// #644 评论 5486167472 问题2：State 2（已 commit）带方向事实。
/// `AlreadyCommitted` 只有在 live index 的内容 hash **等于本次 `prepared_bytes` 的 hash**
/// 时才成立。如果 owner/index 内容是 forward 的 new，而当前调用准备的是 rollback 的 old，
/// 就只能认定"上一阶段 forward commit 已完成"，先清 lock 目录，再 fall through
/// 重新 acquire（写本次 prepared_bytes 并获取 lock），不能直接返回 `AlreadyCommitted`。
/// Missing 路径：`prepared_bytes` 为空切片时，`AlreadyCommitted` 要求 live index 也不存在
///（Missing == Missing）。
pub struct OwnedIndexLock {
    /// `.git/index.lock`（现在是目录，不是 regular file）。
    lock_path: PathBuf,
    /// `lock_path/prepared`：写 prepared bytes（目标 index 内容）。
    prepared_file: PathBuf,
    /// `true` 表示已成功 commit（lock 目录已删走），`drop` 无操作。
    /// `false` 表示未 commit（出错路径），`drop` 清理 lock 目录。
    disarmed: bool,
}

/// #644 评论 5486167472 问题1：owner metadata 格式常量。
///
/// 写入 `.git/index.lock/owner` 的 owner metadata，用于区分本轮 lock 和不同事务的 lock。
/// 外部 Git 创建的 `index.lock` 是 regular file（不是目录），恢复时看到 regular file
/// 就知道是外部 Git 的 lock → `ConcurrentMetadataChanged`，绝不删。
pub const INDEX_LOCK_MARKER: &str = "sujian-index-lock-v1";

/// #644 评论 5486167472 问题1：构造 owner metadata 字节串。
pub fn owner_metadata(owner: &str) -> Vec<u8> {
    format!("{INDEX_LOCK_MARKER}\nowner={owner}\n").into_bytes()
}

/// #644 评论 5486167472 问题1：解析 owner 文件内容，判断是否是本轮 owner。
///
/// 返回 `true` 表示 owner 文件内容是本轮 owner metadata（格式匹配 + owner 匹配）。
/// 返回 `false` 表示无法解析（损坏的 owner 文件）或 owner 不匹配（不同事务的 lock）。
///
/// 注意：此函数只解析 owner metadata 字节串。目录锁模型的归属判断用
/// `lock_dir_belongs_to_owner`，它先检查 lock_path 是否是目录，再读 owner 文件。
pub fn lock_belongs_to_owner(lock_bytes: &[u8], owner: &str) -> bool {
    let Ok(text) = std::str::from_utf8(lock_bytes) else {
        return false;
    };
    let mut lines = text.lines();
    if lines.next() != Some(INDEX_LOCK_MARKER) {
        return false;
    }
    let expected = format!("owner={owner}");
    lines.next() == Some(&expected)
}

/// #644 评论 5486852142 问题1：目录锁模型的归属判断。
///
/// 返回值语义：
/// - `LockOwner::Ours`：lock_path 是目录，且 owner 文件不存在/为空（本轮未完成写入）
///   或 owner 文件内容匹配本轮 owner。可以安全清理（Sujian lock 不可能是外部 Git 的）。
/// - `LockOwner::External`：lock_path 是 regular file（外部 Git 进程的 lock），
///   或 lock_path 是目录但 owner 文件内容不匹配（不同事务的 lock）。绝不碰。
/// - `LockOwner::Absent`：lock_path 不存在。
pub enum LockOwner {
    /// lock_path 不存在。
    Absent,
    /// lock_path 是目录且归属本轮（owner 匹配或 owner 文件不存在/为空）。
    Ours,
    /// lock_path 是 regular file（外部 Git）或是目录但 owner 不匹配（不同事务）。
    External,
}

/// #644 评论 5486852142 问题1：判断 lock_path 的归属。
///
/// - `lock_path` 不存在 → `Absent`。
/// - `lock_path` 是目录 → Sujian lock。读 `lock_path/owner` 文件：
///   - owner 文件不存在/为空 → `Ours`（crash 在 create_dir 和 write owner 之间，
///     可安全清理，因为是 Sujian 目录锁不可能是外部 Git 的）。
///   - owner 文件存在且内容匹配 → `Ours`。
///   - owner 文件存在但不匹配 → `External`（不同事务的 lock）。
/// - `lock_path` 是 regular file → `External`（外部 Git 进程的 lock）。
pub fn lock_dir_belongs_to_owner(lock_path: &Path, owner: &str) -> LockOwner {
    if !lock_path.exists() {
        return LockOwner::Absent;
    }
    if !lock_path.is_dir() {
        // regular file → 外部 Git 进程的 lock。
        return LockOwner::External;
    }
    // lock_path 是目录 → Sujian lock。读 owner 文件判断归属。
    let owner_file = lock_path.join("owner");
    let owner_bytes = fs::read(&owner_file).unwrap_or_default();
    if owner_bytes.is_empty() {
        // owner 文件不存在/为空 → Sujian lock 但未完成写入，可安全清理。
        return LockOwner::Ours;
    }
    if lock_belongs_to_owner(&owner_bytes, owner) {
        LockOwner::Ours
    } else {
        LockOwner::External
    }
}

/// #644 评论 5486852142 问题1：删除 lock 目录（如果存在且是目录）并 fsync 父目录。
///
/// 提取为辅助函数以减少 `acquire` 的嵌套深度。返回 `bool` 表示是否真的删了
///（调用方据此决定是否需要 sync_dir）。
fn remove_lock_dir_if_exists(lock_path: &Path, git_dir: &Path) -> bool {
    if lock_path.is_dir() && fs::remove_dir_all(lock_path).is_ok() {
        let _ = crate::storage::sync_dir(git_dir);
        return true;
    }
    false
}

/// #644 评论 5485518160 修改点 1：`OwnedIndexLock::acquire` 的返回类型。
///
/// 区分"新获取的锁（未提交，需调用方 commit）"和"上次已提交的锁（调用方应跳过 commit）"。
/// `AlreadyCommitted` 表示磁盘状态显示上一次 `commit_rename`/`commit_delete` 已完成
///（lock 不存在 + live index hash == 本次 prepared_bytes hash），调用方应把 lock 视为
/// 已 disarm，不再 commit，直接依据 current index 的 old/new 状态继续恢复。
pub enum AcquireOutcome {
    /// 新获取的锁，调用方需在适当时机调用 `commit_rename`/`commit_delete`。
    NewlyAcquired(OwnedIndexLock),
    /// 上次 acquire 后已成功 commit（rename/delete 已完成），调用方应跳过 commit。
    AlreadyCommitted,
}

impl OwnedIndexLock {
    /// #644 评论 5486167472 问题1+问题2 + #644 评论 5486852142 问题1：恢复已有 ownership → 再创建新 ownership 的两段状态机。
    ///
    /// ## Phase 1：检查磁盘崩溃状态
    ///
    /// lock_path = `.git/index.lock`（目录），index_path = `.git/index`。
    /// 根据 lock_path 类型 + index hash 判断上一次 acquire/commit 死在哪一步：
    ///
    /// - **State 1（已拿锁未 commit）**：lock_path 是目录 + owner 文件不存在/为空或
    ///   owner 匹配本轮。说明上次 acquire 已成功但 commit 未执行。删除 lock 目录，
    ///   fsync `.git`，fall through 到 Phase 2 重新 acquire。
    /// - **State 2（已 commit，且方向匹配）**：lock_path 不存在 + live index hash == 本次
    ///   `prepared_bytes` hash（Missing == Missing 也算）。说明上次 `commit_rename`/`commit_delete`
    ///   已完成且内容方向匹配。返回 `AlreadyCommitted`。
    /// - **State 3（已 commit，但方向不匹配）**：lock_path 不存在 + live index hash != 本次
    ///   `prepared_bytes` hash。说明是另一方向的已提交状态（forward commit 后崩溃，现在
    ///   进入 rollback）。fall through 到 Phase 2 重新 acquire（写本次 prepared_bytes 并获取 lock）。
    /// - **State 4（归属未知/外部 Git）**：lock_path 是 regular file（外部 Git 进程的 lock），
    ///   或 lock_path 是目录但 owner 文件内容不匹配本轮（不同事务的 lock）。返回
    ///   `ConcurrentMetadataChanged`，保留事务，绝不碰 lock。
    ///
    /// ## Phase 2：新建 ownership（目录锁模型）
    ///
    /// 1. `fs::create_dir(&lock_path)` 原子创建 lock 目录（已存在则失败）。
    ///    这一步本身就是原子的 ownership 证明。
    /// 2. 在 `lock_path/owner` 写 owner metadata，fsync。如果失败，清理目录并返回 Err。
    /// 3. 在 `lock_path/prepared` 写 `prepared_bytes`，fsync。如果失败，清理目录并返回 Err。
    ///
    /// 关键安全属性：`create_dir` 成功就是原子的 ownership 证明。即使 crash 在 create_dir 后、
    /// write owner 前，恢复时看到目录就知道是 Sujian lock（不可能是外部 Git 的），可以安全清理。
    /// 不再有"空/半截 lock 被当成外部 Git lock"的窗口。
    pub fn acquire(
        git_dir: &Path,
        owner: &str,
        prepared_bytes: &[u8],
    ) -> std::result::Result<AcquireOutcome, GitFinalizeError> {
        use std::io::Write;

        let lock_path = git_dir.join("index.lock");
        let owner_file = lock_path.join("owner");
        let prepared_file = lock_path.join("prepared");
        let index_path = git_dir.join("index");

        let metadata = owner_metadata(owner);

        // ── Phase 1：检查磁盘崩溃状态 ──
        match lock_dir_belongs_to_owner(&lock_path, owner) {
            LockOwner::Absent => {
                // lock 不存在：检查 index hash 判断是否已 commit（State 2 / State 3）。
                let current_index_hash = if index_path.exists() {
                    let bytes = fs::read(&index_path)
                        .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
                    Some(sha256_bytes(&bytes))
                } else {
                    None
                };
                let prepared_hash = sha256_bytes(prepared_bytes);
                // Missing == Missing 也算 AlreadyCommitted（prepared_bytes 为空 + index 不存在）。
                let index_matches_prepared = match &current_index_hash {
                    Some(h) => *h == prepared_hash,
                    None => prepared_bytes.is_empty(),
                };

                if index_matches_prepared {
                    // State 2：上次 commit 已完成且方向匹配。返回 AlreadyCommitted。
                    return Ok(AcquireOutcome::AlreadyCommitted);
                }
                // State 3：lock 不存在但 index hash != prepared_bytes hash。
                // 可能是另一方向的已提交状态（forward commit 后崩溃，现在 rollback）。
                // fall through 到 Phase 2 重新 acquire。
            }
            LockOwner::Ours => {
                // State 1：上次已拿锁但还没 commit。lock 目录是本事务自己的。
                // 删除 lock 目录，fsync .git，fall through 到 Phase 2 重新 acquire。
                fs::remove_dir_all(&lock_path)
                    .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
                crate::storage::sync_dir(git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
            }
            LockOwner::External => {
                // State 4：lock_path 是 regular file（外部 Git 进程的 lock），
                // 或 lock_path 是目录但 owner 不匹配（不同事务的 lock）。
                // 绝不碰 lock，返回 ConcurrentMetadataChanged。
                return Err(GitFinalizeError::ConcurrentMetadataChanged {
                    reason: "index.lock exists but does not belong to us: \
                             concurrent git process (regular file lock) or different \
                             transaction (directory lock with mismatched owner) is writing index"
                        .to_string(),
                });
            }
        }

        // ── Phase 2：新建 ownership（目录锁模型） ──
        // 1. create_dir 原子创建 lock 目录（已存在则失败）。
        //    这一步本身就是原子的 ownership 证明。
        fs::create_dir(&lock_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_dir(git_dir).map_err(GitFinalizeError::FinalizeFailed)?;

        // 2. 在 lock 目录内创建 owner 文件，写 owner metadata，fsync。
        //    如果失败，清理目录并返回 Err。
        {
            let mut owner_f = match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&owner_file)
            {
                Ok(f) => f,
                Err(e) => {
                    let _ = fs::remove_dir_all(&lock_path);
                    let _ = crate::storage::sync_dir(git_dir);
                    return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(e)));
                }
            };
            owner_f
                .write_all(&metadata)
                .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
            owner_f
                .flush()
                .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
            owner_f
                .sync_all()
                .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
            drop(owner_f);
            crate::storage::sync_parent(&owner_file).map_err(GitFinalizeError::FinalizeFailed)?;
        }

        // 3. 在 lock 目录内创建 prepared 文件，写 prepared_bytes，fsync。
        //    如果失败，清理目录并返回 Err。
        {
            let mut prepared_f = match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&prepared_file)
            {
                Ok(f) => f,
                Err(e) => {
                    let _ = fs::remove_dir_all(&lock_path);
                    let _ = crate::storage::sync_dir(git_dir);
                    return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(e)));
                }
            };
            prepared_f
                .write_all(prepared_bytes)
                .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
            prepared_f
                .flush()
                .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
            prepared_f
                .sync_all()
                .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
            drop(prepared_f);
            crate::storage::sync_parent(&prepared_file)
                .map_err(GitFinalizeError::FinalizeFailed)?;
        }

        Ok(AcquireOutcome::NewlyAcquired(Self {
            lock_path,
            prepared_file,
            disarmed: false,
        }))
    }

    /// 提交（Bytes 路径）：`rename prepared_file → index`（原子提交），然后删 lock 目录。
    /// 每步 fsync 父目录。成功后 `disarm`，`drop` 无操作。
    pub fn commit_rename(
        &mut self,
        index_path: &Path,
    ) -> std::result::Result<(), GitFinalizeError> {
        // rename prepared_file → index（覆盖式 rename，原子提交）。
        fs::rename(&self.prepared_file, index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(index_path).map_err(GitFinalizeError::FinalizeFailed)?;
        // 删 lock 目录，fsync .git。
        fs::remove_dir_all(&self.lock_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(&self.lock_path).map_err(GitFinalizeError::FinalizeFailed)?;
        self.disarmed = true;
        Ok(())
    }

    /// 提交（Missing 路径）：删除 index，然后删除 prepared_file，然后删除 lock 目录，
    /// 每步 fsync 父目录。
    pub fn commit_delete(
        &mut self,
        index_path: &Path,
    ) -> std::result::Result<(), GitFinalizeError> {
        fs::remove_file(index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(index_path).map_err(GitFinalizeError::FinalizeFailed)?;
        fs::remove_file(&self.prepared_file)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(&self.prepared_file)
            .map_err(GitFinalizeError::FinalizeFailed)?;
        fs::remove_dir_all(&self.lock_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        crate::storage::sync_parent(&self.lock_path).map_err(GitFinalizeError::FinalizeFailed)?;
        self.disarmed = true;
        Ok(())
    }
}

impl Drop for OwnedIndexLock {
    fn drop(&mut self) {
        if !self.disarmed {
            // 未 commit（出错路径）：清理 lock 目录（如果还在）。
            // remove_dir_all 会递归删除 owner_file 和 prepared_file。
            let _ = fs::remove_dir_all(&self.lock_path);
        }
        // commit 成功后 lock 目录已被 remove_dir_all 删走；
        // 未 commit 时上面的 remove_dir_all 已清理。无需额外操作。
    }
}

/// #644 评论 5480360027 修复点 3 + #644 评论 5481496190 问题2 +
/// #644 评论 5486167472 问题1 + #644 评论 5486852142 问题1：index 原生锁边界 + 持久 ownership。
///
/// 流程（OwnedIndexLock 目录锁模型）：
/// 1. 在 staging repo 的 .git 目录下生成目标 index 字节（对应 staging HEAD 的 tree）。
/// 2. 用 `OwnedIndexLock::acquire` 把 `.git/index.lock` 创建为**目录**，
///    目录内 `owner` 文件写 owner metadata，`prepared` 文件写目标 index 字节（fsync）。
///    lock 已存在（目录或 regular file）→ `ConcurrentMetadataChanged`，绝不碰别人的 lock。
/// 3. 拿到锁后重新读取 live index，确认仍等于 snapshot.index。
/// 4. 仍一致才 `commit_rename`（rename prepared_file → index，然后删 lock 目录，原子提交 + 释放锁）。
/// 5. 不一致直接返回 `ConcurrentMetadataChanged`，不允许覆盖。
///
/// #644 评论 5486852142 问题1：ownership 是磁盘事实（lock 目录存在性 + owner 文件内容），
/// 即使进程被 SIGKILL，Drop 不执行，恢复时也能通过目录类型 + owner 文件判断 lock 归属。
/// `create_dir` 成功就是原子的 ownership 证明，不再有 create-to-write 窗口。
/// 不依赖 hardlink/inode 比较，Android shared-storage（AOSP FUSE）可落地。
fn install_index_with_lock(
    live_root: &Path,
    _live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    owner: &str,
) -> std::result::Result<(), GitFinalizeError> {
    // 1. 在 staging .git 目录下生成目标 index 字节。
    let target_index_bytes = generate_target_index_bytes(staging_repo, new_oid)?;

    let git_dir = live_root.join(".git");
    let index_path = git_dir.join("index");

    // 2. OwnedIndexLock::acquire：O_EXCL 创建 lock + 写 owner metadata + 写 prepared file。
    //    #644 评论 5485518160 修改点 1：acquire 返回 AcquireOutcome。
    //    - NewlyAcquired：拿到新锁，继续 CAS 检查 + commit。
    //    - AlreadyCommitted：上次 commit_rename 已完成（index 已是目标状态），
    //      跳过 CAS 检查和 commit，直接返回 Ok（index 已是目标状态）。
    let mut lock = match OwnedIndexLock::acquire(&git_dir, owner, &target_index_bytes)? {
        AcquireOutcome::NewlyAcquired(lock) => lock,
        AcquireOutcome::AlreadyCommitted => {
            // 上次 commit_rename 已完成，index 已是目标状态，无需再 commit。
            return Ok(());
        }
    };

    // 3. 拿到锁后重新读取 live index，确认仍等于 snapshot.index。
    let current_index = if index_path.exists() {
        let bytes = fs::read(&index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };
    if !index_snapshot_eq(&current_index, &snapshot.index) {
        // CAS 失败：返回 ConcurrentMetadataChanged。
        // lock 由 OwnedIndexLock::drop 清理（disarmed=false → 清理 lock + owner_file）。
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: "index changed between verify and acquire of index.lock".to_string(),
        });
    }

    // 4. commit_rename：rename owner_file → index，然后删 lock（原子提交 + 释放锁）。
    lock.commit_rename(&index_path)?;

    Ok(())
}

/// #644 评论 5483239422 问题3 + #644 评论 5483920624 问题2/3 +
/// #644 评论 5486167472 问题1 + #644 评论 5486852142 问题1：rollback index 的 lockfile 反向提交边界 + 持久 ownership。
///
/// 与 forward `install_index_with_lock` 同语义（共用 `OwnedIndexLock`），但方向相反：
/// 把 snapshot.index 的旧字节写回 live index。关键安全约束：
/// 1. 用 `OwnedIndexLock::acquire` 把 `.git/index.lock` 创建为**目录**，
///    目录内 `owner` 文件写 owner metadata，`prepared` 文件写 snapshot.index 旧字节。
///    lock 已存在（目录或 regular file）→ `ConcurrentMetadataChanged`，绝不删别人的 lock
///    （外部 Git 进程可能正在用）。
/// 2. 拿到锁后重新读 index 确认仍等于 `expected_new_sha256`（调用方已确认
///    current==new，但拿锁期间可能被并发改掉，需复验）。不等则返回 Err
///    （并发变化，保留 transaction；lock 由 OwnedIndexLock::drop 清理）。
/// 3. Bytes 路径：`commit_rename`（rename prepared_file → index，然后删 lock 目录，原子恢复 + 释放锁）。
///    Missing 路径：`commit_delete`（删 index → 删 prepared_file → 删 lock 目录，每步 fsync 父目录）。
/// 4. ownership 是磁盘事实（lock 目录存在性 + owner 文件内容），即使 SIGKILL
///    后 Drop 不执行，恢复时也能通过目录类型 + owner 文件判断 lock 归属。
///    `create_dir` 成功就是原子的 ownership 证明，不再有 create-to-write 窗口。
///
/// 调用方已保证进入此函数前 `current_index == new`（plan.new_index_sha256 命中）。
fn rollback_index_via_lockfile(
    index_path: &Path,
    lock_path: &Path,
    expected_new_sha256: [u8; 32],
    snapshot_index: &IndexSnapshot,
    owner: &str,
) -> Result<()> {
    // 1. OwnedIndexLock::acquire：O_EXCL 创建 lock + 写 owner metadata + 写 prepared file
    //   （snapshot.index 旧字节）。lock 已存在 → ConcurrentMetadataChanged（绝不删别人的 lock）。
    //    #644 评论 5485518160 修改点 1：acquire 返回 AcquireOutcome。
    //    - NewlyAcquired：拿到新锁，继续 CAS 复验 + commit。
    //    - AlreadyCommitted：上次反向 commit_rename/commit_delete 已完成
    //      （index 已恢复到 snapshot.index），跳过 commit，直接返回 Ok。
    let git_dir = index_path.parent().ok_or_else(|| {
        crate::Error::Io(std::io::Error::other(
            "rollback_git_finalize: index_path has no parent (.git dir)",
        ))
    })?;
    let prepared_bytes: &[u8] = match snapshot_index {
        IndexSnapshot::Bytes(b) => b,
        IndexSnapshot::Missing => &[],
    };
    let mut lock =
        match OwnedIndexLock::acquire(git_dir, owner, prepared_bytes).map_err(|e| match e {
            GitFinalizeError::ConcurrentMetadataChanged { reason } => {
                crate::Error::Io(std::io::Error::other(format!(
                    "rollback_git_finalize: index.lock exists — refusing to delete external \
                     git process lock; concurrent git operation in progress ({}), preserving \
                     transaction for next recovery",
                    reason
                )))
            }
            GitFinalizeError::FinalizeFailed(inner) => inner,
            GitFinalizeError::RollbackFailed { .. } => crate::Error::Io(std::io::Error::other(
                "rollback_git_finalize: unexpected RollbackFailed from OwnedIndexLock::acquire",
            )),
        })? {
            AcquireOutcome::NewlyAcquired(lock) => lock,
            AcquireOutcome::AlreadyCommitted => {
                // 上次反向 commit 已完成（index 已恢复到 snapshot.index），无需再 commit。
                return Ok(());
            }
        };
    // lock_path 仅用于诊断/一致性检查，确认 OwnedIndexLock 用的 lock 路径与调用方一致。
    debug_assert_eq!(lock_path, &git_dir.join("index.lock"));

    // 2. 拿到锁后重新读 live index，计算 SHA-256，确认仍等于 expected_new_sha256。
    //    若被并发改掉（current != expected_new），返回 Err
    //    （lock 由 OwnedIndexLock::drop 清理：disarmed=false → 清理 lock + owner_file）。
    let current_index = if index_path.exists() {
        let bytes = fs::read(index_path)?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };
    let current_is_new = match &current_index {
        IndexSnapshot::Bytes(b) => sha256_bytes(b) == expected_new_sha256,
        IndexSnapshot::Missing => false,
    };
    if !current_is_new {
        return Err(crate::Error::Io(std::io::Error::other(
            "rollback_git_finalize: index hash changed after acquiring lock (concurrent \
             modification) — cleaned our lock, preserving transaction for next recovery",
        )));
    }

    // 3. 提交：Bytes 路径 commit_rename，Missing 路径 commit_delete。
    match snapshot_index {
        IndexSnapshot::Bytes(_) => {
            lock.commit_rename(index_path).map_err(|e| match e {
                GitFinalizeError::FinalizeFailed(inner) => inner,
                GitFinalizeError::ConcurrentMetadataChanged { reason } => {
                    crate::Error::Io(std::io::Error::other(reason))
                }
                GitFinalizeError::RollbackFailed { .. } => crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: unexpected RollbackFailed from commit_rename",
                )),
            })?;
        }
        IndexSnapshot::Missing => {
            // #644 评论 5483920624 问题3：Missing 路径在持有自己 lock 期间完成：
            // 重新验证 current==expected new（上方已完成）→ 删 index → 删 lock → fsync 父目录。
            lock.commit_delete(index_path).map_err(|e| match e {
                GitFinalizeError::FinalizeFailed(inner) => inner,
                GitFinalizeError::ConcurrentMetadataChanged { reason } => {
                    crate::Error::Io(std::io::Error::other(reason))
                }
                GitFinalizeError::RollbackFailed { .. } => crate::Error::Io(std::io::Error::other(
                    "rollback_git_finalize: unexpected RollbackFailed from commit_delete",
                )),
            })?;
        }
    }

    Ok(())
}

/// #644 评论 5480360027：在 staging repo 中生成目标 index 字节（对应 staging HEAD 的 tree）。
///
/// 在 staging .git 目录下创建临时 index 文件，用 `Index::open` + `read_tree` + `write`
/// 生成字节，读取后删除临时文件。
fn generate_target_index_bytes(
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
) -> std::result::Result<Vec<u8>, GitFinalizeError> {
    let new_commit = staging_repo.find_commit(new_oid).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: find commit: {e}"
        ))))
    })?;
    let new_tree = new_commit.tree().map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: find tree: {e}"
        ))))
    })?;

    // 在 staging .git 目录下创建临时 index 文件。
    let staging_git_dir = staging_repo.path().to_path_buf();
    let tmp_id = uuid::Uuid::new_v4().to_string();
    let tmp_index_path = staging_git_dir.join(format!("index.sujian-tmp-{}", tmp_id));
    let tmp_index_guard = TmpFileGuard::new(tmp_index_path.clone());

    let mut index = git2::Index::open(&tmp_index_path).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: open tmp index: {e}"
        ))))
    })?;
    index.read_tree(&new_tree).map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: read_tree: {e}"
        ))))
    })?;
    index.write().map_err(|e| {
        GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
            "generate_target_index_bytes: write tmp index: {e}"
        ))))
    })?;

    let bytes = fs::read(&tmp_index_path)
        .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
    drop(tmp_index_guard);

    Ok(bytes)
}

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
fn read_ref_snapshot(
    repo: &git2::Repository,
    ref_name: &str,
) -> std::result::Result<RefSnapshot, crate::Error> {
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
///
/// #644 评论 5480360027 修复点 5：严格错误传播。
/// `references()?` 而非 `if let Ok(...)`，循环里 `reference?` 而非 `flatten()`。
/// 任何错误都向上传播，进入本轮 finalize 失败/rollback，不能静默跳过。
#[allow(clippy::excessive_nesting)]
fn sync_remote_refs(
    live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    snapshot_refs: &std::collections::BTreeMap<String, RefSnapshot>,
) -> Result<()> {
    let staging_refs = staging_repo.references().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "sync_remote_refs: failed to iterate staging references: {e}"
        )))
    })?;
    for reference in staging_refs {
        let reference = reference.map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "sync_remote_refs: staging reference iterator error: {e}"
            )))
        })?;
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
            }
            RefSnapshot::Existed { oid } => {
                let old_oid = git2::Oid::from_str(oid).map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "sync_remote_refs: invalid old oid for {}: {e}",
                        name
                    )))
                })?;
                // #644 评论 5481496190 问题1：force=true 让 libgit2 能更新已存在的 ref。
                // current_id=old_oid 提供 CAS 保护，current != old_oid 时返回 EMODIFIED。
                live_repo
                    .reference_matching(
                        name,
                        target,
                        true,
                        old_oid,
                        "sync: update remote-tracking ref from staging",
                    )
                    .map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "sync_remote_refs: CAS update {} failed (old={} new={}): {}",
                            name, old_oid, target, e
                        )))
                    })?;
            }
            RefSnapshot::Symbolic { .. } => {
                // remote ref 不应是 symbolic，跳过（不覆盖）。
                continue;
            }
        }
    }
    Ok(())
}

/// 更新 live 的 index 为 staging HEAD 对应的 tree（不 checkout 工作区）。
///
/// #644 评论 5486167472 问题3：libgit2 的 `GIT_OPT_ENABLE_FSYNC_GITDIR` 已通过
/// `storage::git_runtime::configure()` 在所有 target 统一启用（经 `libgit2-sys`
/// 直接依赖暴露 FFI），libgit2 自己写 ODB/ref 已有 durable barrier。
/// 此处 `live_index.write()` 后的显式 fsync index 文件 + 父目录保留为
/// 事务自身的边界（index 是 finalize 的关键提交点，额外 fsync 不冲突）。
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

    // #644 评论 5486167472 问题3：libgit2 fsync-gitdir 已由 git_runtime::configure()
    // 统一启用，此处显式 fsync index 文件 + 父目录保留为事务自身的边界
    //（index 是 finalize 的关键提交点），保证 index 内容和目录项持久可见。
    let index_path = live_repo.path().join("index");
    if index_path.exists() {
        let index_file = std::fs::File::open(&index_path)?;
        index_file.sync_all()?;
        drop(index_file);
        crate::storage::sync_parent(&index_path)?;
    }

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
///
/// #644 评论 5485518160 修改点 3：durable recursive copy。
/// 普通文件 copy 后对目标文件 `sync_all` + 目标父目录 `sync_dir`（复用
/// `crate::storage::durable_copy_file`）。每层目录 bottom-up `sync_dir`
///（递归返回后 sync 当前 dst 目录），保证目录项持久可见。
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            // #644 评论 5485518160 修改点 3：durable copy — copy 后 fsync 文件 + 父目录。
            crate::storage::durable_copy_file(&src_path, &dst_path)?;
        }
    }
    // bottom-up：递归返回后 sync 当前 dst 目录，持久化目录项。
    crate::storage::sync_dir(dst)?;
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

        let (snapshot, plan) =
            prepare_git_finalize(&live, &GitSeedState::NotGitRepo, &staging).unwrap();
        assert!(matches!(snapshot.head, RefSnapshot::DidNotExist));
        assert!(snapshot.refs.is_empty());
        assert!(matches!(snapshot.index, IndexSnapshot::Missing));
        assert!(!snapshot.repo_existed);
        // #644 评论 5480360027：NotGitRepo 路径 plan.repo_create == true。
        assert!(plan.repo_create);
        // staging 无 .git → 无目标 index hash。
        assert!(plan.new_index_sha256.is_none());
        assert!(plan.ref_plans.is_empty());
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
        let (snapshot, plan) = prepare_git_finalize(&live, &seed, &staging).unwrap();
        assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
        assert!(snapshot.refs.contains_key("refs/heads/main"));
        assert!(snapshot.repo_existed);
        // #644 评论 5480360027：Unborn 路径 plan.repo_create == false。
        assert!(!plan.repo_create);
        // staging 无 .git → 无目标 index hash，无 ref_plans。
        assert!(plan.new_index_sha256.is_none());
        assert!(plan.ref_plans.is_empty());
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
        let (snapshot, plan) = prepare_git_finalize(&live, &seed, &staging).unwrap();
        assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
        assert!(matches!(snapshot.index, IndexSnapshot::Bytes(_)));
        assert!(snapshot.refs.contains_key("refs/heads/main"));
        assert!(snapshot.repo_existed);
        // #644 评论 5480360027：Existing 路径 plan.repo_create == false。
        assert!(!plan.repo_create);
        // staging 无 .git → 无目标 index hash，无 ref_plans。
        assert!(plan.new_index_sha256.is_none());
        assert!(plan.ref_plans.is_empty());
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
        let (snapshot, _plan) = prepare_git_finalize(&live, &seed, &staging).unwrap();
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
        // #644 评论 5480360027：CAS-based rollback 使用 write-ahead plan。
        // #644 评论 5484539222 缺陷1：rollback 反向恢复路径需要 index_lock_owner 做 OwnedIndexLock。
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: Some(sha256_bytes(
                &fs::read(live.join(".git").join("index")).unwrap(),
            )),
            ref_plans: Vec::new(),
            repo_create_owner: None,
            index_lock_owner: Some(uuid::Uuid::new_v4().to_string()),
        };
        rollback_git_finalize(&live, &snapshot, &plan).unwrap();
        let restored_index = fs::read(live.join(".git").join("index")).unwrap();
        assert_eq!(restored_index, original_index);
    }

    /// #644 评论 5480360027：验证 write-ahead plan 在 prepare 阶段完整生成。
    /// staging 有 .git + HEAD 时，plan.new_index_sha256 和 ref_plans 应非空。
    #[test]
    fn prepare_generates_complete_plan_with_staging() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        let _live_repo = git2::Repository::init(&live).unwrap();

        // staging 有 .git + HEAD + 一个 commit。
        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("a.txt"), "hello").unwrap();
        let staging_repo = git2::Repository::init(&staging).unwrap();
        let mut index = staging_repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = staging_repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        // 显式用 refs/heads/main branch。
        let commit_oid = staging_repo
            .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        staging_repo
            .reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
            .unwrap();

        let seed = GitSeedState::Unborn {
            head_ref: "refs/heads/main".to_string(),
        };
        let (snapshot, plan) = prepare_git_finalize(&live, &seed, &staging).unwrap();
        // plan 应有目标 index hash。
        assert!(
            plan.new_index_sha256.is_some(),
            "plan should have new_index_sha256 when staging has HEAD"
        );
        // plan 应有 ref_plans（创建 refs/heads/main）。
        assert_eq!(plan.ref_plans.len(), 1);
        assert_eq!(plan.ref_plans[0].0, "refs/heads/main");
        assert!(plan.ref_plans[0].1.is_none()); // old_oid = None (unborn)
        assert_eq!(plan.ref_plans[0].2, commit_oid.to_string()); // new_oid
                                                                 // snapshot 仍正确。
        assert!(matches!(snapshot.head, RefSnapshot::Symbolic { .. }));
        assert!(snapshot.repo_existed);
    }

    /// #644 评论 5480360027 修复点 3：验证 index lock 边界。
    /// 如果 .git/index.lock 已存在，install_index_with_lock 应返回
    /// ConcurrentMetadataChanged，不覆盖。
    #[test]
    fn index_lock_prevents_concurrent_write() {
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

        // staging 有 .git + HEAD。
        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("a.txt"), "hello").unwrap();
        let staging_repo = git2::Repository::init(&staging).unwrap();
        let mut s_index = staging_repo.index().unwrap();
        s_index.add_path(std::path::Path::new("a.txt")).unwrap();
        s_index.write().unwrap();
        let s_tree_oid = s_index.write_tree().unwrap();
        let s_tree = staging_repo.find_tree(s_tree_oid).unwrap();
        staging_repo
            .commit(Some("HEAD"), &sig, &sig, "init", &s_tree, &[])
            .unwrap();

        // 模拟并发：创建 .git/index.lock。
        let lock_path = live.join(".git").join("index.lock");
        fs::write(&lock_path, b"concurrent").unwrap();

        // install_index_with_lock 应检测到 lock 已存在，返回 ConcurrentMetadataChanged。
        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::Symbolic {
                target: "refs/heads/main".to_string(),
            },
            refs: std::collections::BTreeMap::new(),
            index: IndexSnapshot::Missing,
            repo_existed: true,
        };
        let staging_head = staging_repo.head().unwrap();
        let new_oid = staging_head.target().unwrap();
        let result = install_index_with_lock(
            &live,
            &repo,
            &staging_repo,
            new_oid,
            &snapshot,
            "test-owner",
        );
        assert!(matches!(
            result,
            Err(GitFinalizeError::ConcurrentMetadataChanged { .. })
        ));

        // 清理 lock 文件。
        let _ = fs::remove_file(&lock_path);
    }

    /// #644 评论 5480360027 修复点 4：验证 ref 反向 CAS rollback。
    /// 更新型 ref rollback 用 reference_matching，不 force 覆盖并发修改。
    #[test]
    fn rollback_ref_uses_reverse_cas() {
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
        // 创建第一个 commit（old_oid）。
        let old_oid = repo
            .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        repo.reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
            .unwrap();

        // 创建第二个 commit（new_oid），模拟 finalize 写入。
        fs::write(live.join("b.txt"), "world").unwrap();
        let mut index2 = repo.index().unwrap();
        index2.add_path(std::path::Path::new("b.txt")).unwrap();
        index2.write().unwrap();
        let tree2_oid = index2.write_tree().unwrap();
        let tree2 = repo.find_tree(tree2_oid).unwrap();
        let old_commit = repo.find_commit(old_oid).unwrap();
        let new_oid = repo
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                "second",
                &tree2,
                &[&old_commit],
            )
            .unwrap();

        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::Symbolic {
                target: "refs/heads/main".to_string(),
            },
            refs: std::collections::BTreeMap::from([(
                "refs/heads/main".to_string(),
                RefSnapshot::Existed {
                    oid: old_oid.to_string(),
                },
            )]),
            index: IndexSnapshot::Missing,
            repo_existed: true,
        };

        // plan 记录 old_oid -> new_oid。
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: None,
            ref_plans: vec![(
                "refs/heads/main".to_string(),
                Some(old_oid.to_string()),
                new_oid.to_string(),
            )],
            repo_create_owner: None,
            index_lock_owner: None,
        };

        // rollback 应成功（current == new_oid，反向 CAS 回 old_oid）。
        rollback_git_finalize(&live, &snapshot, &plan).unwrap();

        // 验证 ref 已恢复到 old_oid（反向 CAS 成功）。
        let repo2 = git2::Repository::open(&live).unwrap();
        let ref_head = repo2.find_reference("refs/heads/main").unwrap();
        assert_eq!(ref_head.target(), Some(old_oid));
    }

    /// #644 评论 5480360027 修复点 2：验证 plan 在写 live 前完整落盘。
    /// prepare_git_finalize 返回的 plan 不依赖 finalize 过程中的内存状态。
    #[test]
    fn plan_is_complete_before_writing_live() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();

        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("a.txt"), "hello").unwrap();
        let staging_repo = git2::Repository::init(&staging).unwrap();
        let mut index = staging_repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = staging_repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        // 显式用 refs/heads/main branch。
        let commit_oid = staging_repo
            .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        // 设置 HEAD 指向 refs/heads/main。
        staging_repo
            .reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
            .unwrap();

        // NotGitRepo 路径：plan 应完整（repo_create=true, new_index_sha256=Some, ref_plans 非空）。
        let (snapshot, plan) =
            prepare_git_finalize(&live, &GitSeedState::NotGitRepo, &staging).unwrap();
        assert!(plan.repo_create);
        assert!(plan.new_index_sha256.is_some());
        assert_eq!(plan.ref_plans.len(), 1);
        assert_eq!(plan.ref_plans[0].0, "refs/heads/main");
        assert!(plan.ref_plans[0].1.is_none()); // NotGitRepo → old=None
        assert_eq!(plan.ref_plans[0].2, commit_oid.to_string());
        // snapshot 是最小快照。
        assert!(!snapshot.repo_existed);
        assert!(matches!(snapshot.index, IndexSnapshot::Missing));
    }

    /// #644 评论 5483239422 问题2：Git rollback 不幂等。
    ///
    /// 复现策略：构造 live repo，index 已被第一次 rollback 恢复成 snapshot.index（old），
    /// plan.new_index_sha256 指向 new_index（不同于当前）。第二次调用
    /// `rollback_git_finalize` 应 no-op（current == old → AlreadyReverted）。
    /// - 当前行为：`index_is_ours = (current == new)` 为 false，lock 不存在，
    ///   走 `else` 分支返回 Err（index CAS miss），事务永久卡死。
    /// - 预期行为：current == old(snapshot) → no-op，返回 Ok(Reverted)。
    ///
    /// 此测试断言预期行为（第二次 rollback 应成功），当前代码下断言失败。
    #[test]
    fn rollback_should_be_idempotent_when_index_already_reverted() {
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

        // original index bytes（snapshot.index）
        let original_index = fs::read(live.join(".git").join("index")).unwrap();

        // 修改 index 得到 new_index（plan.new）
        fs::write(live.join("b.txt"), "new").unwrap();
        let mut index2 = repo.index().unwrap();
        index2.add_path(std::path::Path::new("b.txt")).unwrap();
        index2.write().unwrap();
        let new_index = fs::read(live.join(".git").join("index")).unwrap();
        let new_index_hash = sha256_bytes(&new_index);

        // 模拟第一次 rollback 已执行：把 index 恢复成 original（== snapshot.index）。
        fs::write(live.join(".git").join("index"), &original_index).unwrap();

        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::Symbolic {
                target: "refs/heads/main".to_string(),
            },
            refs: std::collections::BTreeMap::new(),
            index: IndexSnapshot::Bytes(original_index),
            repo_existed: true,
        };
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: Some(new_index_hash),
            ref_plans: Vec::new(),
            repo_create_owner: None,
            index_lock_owner: None,
        };

        // 第二次 rollback：current index == original (== snapshot.index)。
        // 预期：no-op，返回 Ok(Reverted)。
        // 当前：index CAS miss（current != new），返回 Err。
        let result = rollback_git_finalize(&live, &snapshot, &plan);
        assert!(
            result.is_ok(),
            "rollback should be idempotent when index already reverted to snapshot; \
             current code returns Err (index CAS miss because current == old is not \
             recognized as AlreadyReverted), permanently stucking the transaction"
        );
    }

    /// #644 评论 5483239422 问题2（ref 部分）：ref rollback 不幂等。
    ///
    /// 复现策略：构造 live repo，ref 已被第一次 rollback 反向 CAS 回 old_oid，
    /// plan 记录 old_oid -> new_oid。第二次调用 `rollback_git_finalize` 应 no-op
    /// （current == old → no-op）。
    /// - 当前行为：`old_oid=Some` 时只接受 `current == new_oid`，current == old_oid
    ///   走 else 分支返回 Err（ref CAS miss），事务永久卡死。
    /// - 预期行为：current == old → no-op，返回 Ok(Reverted)。
    ///
    /// 此测试断言预期行为（第二次 rollback 应成功），当前代码下断言失败。
    #[test]
    fn rollback_should_be_idempotent_when_ref_already_reverted() {
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
        let old_oid = repo
            .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        repo.reference_symbolic("HEAD", "refs/heads/main", true, "test: set HEAD")
            .unwrap();

        // 创建第二个 commit（new_oid），模拟 finalize 写入。
        fs::write(live.join("b.txt"), "world").unwrap();
        let mut index2 = repo.index().unwrap();
        index2.add_path(std::path::Path::new("b.txt")).unwrap();
        index2.write().unwrap();
        let tree2_oid = index2.write_tree().unwrap();
        let tree2 = repo.find_tree(tree2_oid).unwrap();
        let old_commit = repo.find_commit(old_oid).unwrap();
        let new_oid = repo
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                "second",
                &tree2,
                &[&old_commit],
            )
            .unwrap();

        // 模拟第一次 rollback 已执行：把 ref 反向 CAS 回 old_oid。
        repo.reference_matching(
            "refs/heads/main",
            old_oid,
            true,
            new_oid,
            "simulate first rollback",
        )
        .unwrap();

        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::Symbolic {
                target: "refs/heads/main".to_string(),
            },
            refs: std::collections::BTreeMap::from([(
                "refs/heads/main".to_string(),
                RefSnapshot::Existed {
                    oid: old_oid.to_string(),
                },
            )]),
            index: IndexSnapshot::Missing,
            repo_existed: true,
        };
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: None,
            ref_plans: vec![(
                "refs/heads/main".to_string(),
                Some(old_oid.to_string()),
                new_oid.to_string(),
            )],
            repo_create_owner: None,
            index_lock_owner: None,
        };

        // 第二次 rollback：current ref == old_oid (== snapshot ref)。
        // 预期：no-op，返回 Ok(Reverted)。
        // 当前：ref CAS miss（current == old != new），返回 Err。
        let result = rollback_git_finalize(&live, &snapshot, &plan);
        assert!(
            result.is_ok(),
            "rollback should be idempotent when ref already reverted to old_oid; \
             current code returns Err (ref CAS miss because current == old is not \
             recognized as no-op), permanently stucking the transaction"
        );
    }

    /// #644 评论 5483239422 问题3：rollback index 误删外部 Git 进程的 index.lock。
    ///
    /// 复现策略：构造 live index == plan.new（本轮 install 成功），另一个正常 Git
    /// 进程创建自己的 `.git/index.lock`。调用 `rollback_git_finalize`：
    /// - 当前行为：`index_is_ours = true`，恢复 snapshot.index，然后
    ///   `if lock_path.exists() { let _ = fs::remove_file(&lock_path); }` 删 lock，
    ///   破坏外部 git add/checkout/merge。
    /// - 预期行为：不应删别人的 lock。应走真正的 lockfile 反向提交边界：用
    ///   `create_new(true)` 自己获取锁，lock 已存在则返回 ConcurrentChanged。
    ///
    /// 此测试断言预期行为（lock 应仍存在），当前代码下断言失败。
    #[test]
    fn rollback_should_not_delete_external_index_lock() {
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

        // original index bytes（snapshot.index）
        let original_index = fs::read(live.join(".git").join("index")).unwrap();

        // 修改 index 得到 new_index（模拟本轮 install 成功，index == plan.new）
        fs::write(live.join("b.txt"), "new").unwrap();
        let mut index2 = repo.index().unwrap();
        index2.add_path(std::path::Path::new("b.txt")).unwrap();
        index2.write().unwrap();
        let new_index = fs::read(live.join(".git").join("index")).unwrap();
        let new_index_hash = sha256_bytes(&new_index);

        // 另一个正常 Git 进程创建自己的 index.lock（不属于本轮）。
        let lock_path = live.join(".git").join("index.lock");
        fs::write(&lock_path, b"external-git-process-lock-marker").unwrap();

        let snapshot = GitMetadataSnapshot {
            head: RefSnapshot::Symbolic {
                target: "refs/heads/main".to_string(),
            },
            refs: std::collections::BTreeMap::new(),
            index: IndexSnapshot::Bytes(original_index),
            repo_existed: true,
        };
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: Some(new_index_hash),
            ref_plans: Vec::new(),
            repo_create_owner: None,
            // #644 评论 5484539222 缺陷1：反向恢复路径需要 owner 做 OwnedIndexLock。
            // acquire 时检测到外部 lock 已存在 → ConcurrentMetadataChanged，不删外部 lock。
            index_lock_owner: Some(uuid::Uuid::new_v4().to_string()),
        };

        // rollback：current index == new_index，index_is_ours = true。
        // 当前：恢复 original，然后删 lock（危险）。
        // 预期：不应删别人的 lock。
        let _ = rollback_git_finalize(&live, &snapshot, &plan);

        assert!(
            lock_path.exists(),
            "rollback must NOT delete index.lock belonging to another Git process; \
             current code removes it via `let _ = fs::remove_file(&lock_path)` after \
             restoring index, breaking external git add/checkout/merge"
        );
    }
}
