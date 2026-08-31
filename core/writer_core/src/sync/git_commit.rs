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

    let branch_name = if let Some(name) = staging_head.shorthand() {
        name.to_string()
    } else {
        "main".to_string()
    };

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
        });
    };

    let mut ref_plans: Vec<(String, Option<String>, String)> = Vec::new();

    match seed_state {
        GitSeedState::NotGitRepo => {
            // NotGitRepo 路径：创建 refs/heads/<branch>。
            let branch_name = if staging_plan.staging_branch_name.is_empty() {
                "main"
            } else {
                &staging_plan.staging_branch_name
            };
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

/// #644 评论 5480360027：CAS-based rollback，根据 plan 的 old_oid/new_oid 做反向 CAS。
///
/// 恢复时根据当前值判断某一步是否真的发生：
/// - current == new_oid → 这一步已执行，反向 CAS 回 old_oid；
/// - current == old_oid → 这一步没执行，无需 rollback；
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
        return Ok(GitRollbackOutcome::Reverted);
    }

    // 2. repo_create=false：index + refs rollback。
    //    #644 评论 5481496190 问题2：同时处理可能残留的 .git/index.lock。
    //    install_index_with_lock 用 lockfile rename 模型，崩溃窗口是：
    //    - lock 写完但 rename 未完成 → lock 拋留，index 未变（仍是旧内容）。
    //    - rename 完成 → lock 已消失，index 是新内容。
    if let Some(expected_hash) = plan.new_index_sha256 {
        let index_path = live_root.join(".git").join("index");
        let lock_path = live_root.join(".git").join("index.lock");

        let index_is_ours = if index_path.exists() {
            let current_bytes = fs::read(&index_path)?;
            sha256_bytes(&current_bytes) == expected_hash
        } else {
            false
        };

        if index_is_ours {
            // CAS 命中：本轮 install 已完成 rename（index 是新内容），恢复 snapshot.index。
            match &snapshot.index {
                IndexSnapshot::Bytes(original_bytes) => {
                    crate::storage::atomic_write_bytes(&index_path, original_bytes)?;
                }
                IndexSnapshot::Missing => {
                    if index_path.exists() {
                        fs::remove_file(&index_path)?;
                    }
                }
            }
            // 恢复后清理可能残留的 lock（本轮崩溃窗口的 lock，index 已恢复）。
            if lock_path.exists() {
                let _ = fs::remove_file(&lock_path);
            }
        } else if lock_path.exists() {
            // index 不是新内容但 lock 存在 → lockfile rename 模型下 lock 是本轮
            // create_new 独占创建的（别人无法同时创建），属于本轮 install 的崩溃窗口
            //（lock 写完但 rename 未完成，index 仍是旧内容）。
            // 清理残留 lock，index 未变无需恢复。
            let _ = fs::remove_file(&lock_path);
            log::warn!(
                "rollback_git_finalize: cleaned stale index.lock left by crashed install \
                 (index unchanged, lock was ours by create_new exclusivity)"
            );
        } else {
            // #644 评论 5482310913 问题3：index CAS miss 且 lock 不存在 → 真正并发修改
            //（既不是 snapshot.index 也不是 plan.new_index_sha256）。
            // 返回 Err 让上层保留 transaction，不继续回滚文件。
            return Err(crate::Error::Io(std::io::Error::other(
                "rollback_git_finalize: index CAS miss (concurrent modification, \
                 current matches neither snapshot.index nor plan.new_index_sha256) — \
                 refusing to continue rollback to preserve transaction for next recovery",
            )));
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
            // Ref was created by us (DidNotExist -> new_oid).
            // Only delete if it still equals what we wrote.
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
                    // #644 评论 5482310913 问题3：ref CAS miss → 真正并发新状态。
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

            // Ref was updated by us (old_oid -> new_oid).
            // #644 评论 5480360027 修复点 4：用反向 CAS `reference_matching`。
            (Some(old_oid_str), Ok(current_ref)) => {
                if current_ref.target() == Some(new_oid) {
                    let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                        crate::Error::Io(std::io::Error::other(format!(
                            "rollback_git_finalize: invalid old_oid for {}: {e}",
                            ref_name
                        )))
                    })?;
                    // 反向 CAS：只有 current == new_oid 时才写 old_oid。
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
                    // #644 评论 5482310913 问题3：ref CAS miss → 真正并发新状态。
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "rollback_git_finalize: ref {} CAS miss (current != new_oid {}) — \
                         concurrent modification, refusing to continue rollback to preserve \
                         transaction",
                        ref_name, new_oid
                    ))));
                }
            }
            (Some(_), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // Ref was deleted by someone else after we wrote it.
                // Don't recreate it — the deletion is a newer change。
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
    _plan: &GitFinalizePlan,
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

    // #644 评论 5480360027 修复点 3：index 原生锁边界。
    // 先在临时目录生成目标 index，获取 .git/index.lock，
    // 拿到锁后重新读取 live index 确认仍等于 snapshot，一致才原子安装。
    install_index_with_lock(live_root, &live_repo, staging_repo, new_oid, snapshot)?;

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
    _plan: &GitFinalizePlan,
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

    // #644 评论 5480360027 修复点 3：index 原生锁边界。
    install_index_with_lock(live_root, &live_repo, staging_repo, new_oid, snapshot)?;

    // #644 评论 5480360027 修复点 5：sync_remote_refs 严格错误传播。
    sync_remote_refs(&live_repo, staging_repo, &snapshot.refs)?;

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
    _plan: &GitFinalizePlan,
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

    // #644 评论 5480360027 修复点 3：index 原生锁边界。
    // #644 评论 5481496190 问题4：ConcurrentMetadataChanged 原样向上传播，不降级。
    install_index_with_lock(live_root, &live_repo, staging_repo, new_oid, snapshot)?;

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

/// #644 评论 5480360027 修复点 3 + #644 评论 5481496190 问题2：index 原生锁边界。
///
/// 流程（lockfile rename 模型，单一写入链）：
/// 1. 在 staging repo 的 .git 目录下生成目标 index 字节（对应 staging HEAD 的 tree）。
/// 2. 获取 live `.git/index.lock` 独占锁（Git 原生 index lock 约定路径）。
/// 3. 拿到锁后重新读取 live index，确认仍等于 snapshot.index。
/// 4. 仍一致才把目标 index 字节直接写入 lock 文件，fsync 后 rename lock → index。
///    rename 成功即提交并释放锁（lock 不再存在，index 是新内容）。
/// 5. 不一致直接返回 `ConcurrentMetadataChanged`，不允许覆盖。
///
/// #644 评论 5481496190 问题2：不再用 IndexLockGuard（空 lock 文件）+
/// atomic_write_bytes(index) 双写入链。旧模型崩溃窗口在 atomic_write 完成后、
/// Drop 删除 lock 前，新 index 已写入但空 .git/index.lock 永久残留。新模型
/// 把内容直接写进 lock 文件，rename lock → index 作为提交和解锁，消除崩溃窗口：
/// - rename 前 crash：lock 拋留但 index 未变，rollback 清理 lock 即可。
/// - rename 后 crash：lock 已消失，index 已更新，无需清理。
fn install_index_with_lock(
    live_root: &Path,
    _live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
) -> std::result::Result<(), GitFinalizeError> {
    use std::io::Write;

    // 1. 在 staging .git 目录下生成目标 index 字节。
    let target_index_bytes = generate_target_index_bytes(staging_repo, new_oid)?;

    // 2. 获取 live .git/index.lock 独占锁（create_new，文件已存在返回并发错误）。
    let lock_path = live_root.join(".git").join("index.lock");
    let mut lock_file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(GitFinalizeError::ConcurrentMetadataChanged {
                reason: "index.lock exists: concurrent git process is writing index".to_string(),
            });
        }
        Err(e) => return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(e))),
    };

    // 3. 拿到锁后重新读取 live index，确认仍等于 snapshot.index。
    let index_path = live_root.join(".git").join("index");
    let current_index = if index_path.exists() {
        let bytes = fs::read(&index_path)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        IndexSnapshot::Bytes(bytes)
    } else {
        IndexSnapshot::Missing
    };
    if !index_snapshot_eq(&current_index, &snapshot.index) {
        // CAS 失败：清理我们创建的 lock 并返回 ConcurrentMetadataChanged。
        let _ = fs::remove_file(&lock_path);
        return Err(GitFinalizeError::ConcurrentMetadataChanged {
            reason: "index changed between verify and acquire of index.lock".to_string(),
        });
    }

    // 4. 把目标 index 字节直接写入 lock 文件，fsync 后 rename lock → index。
    //    写入或 rename 失败时清理 lock，不留半写入状态。
    if let Err(e) = lock_file.write_all(&target_index_bytes) {
        let _ = fs::remove_file(&lock_path);
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(e)));
    }
    if let Err(e) = lock_file.flush() {
        let _ = fs::remove_file(&lock_path);
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(e)));
    }
    if let Err(e) = lock_file.sync_all() {
        let _ = fs::remove_file(&lock_path);
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(e)));
    }
    drop(lock_file);

    // rename lock → index（原子提交 + 释放锁）。
    if let Err(e) = fs::rename(&lock_path, &index_path) {
        let _ = fs::remove_file(&lock_path);
        return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(e)));
    }

    // fsync 父目录持久化目录项（Unix）。
    #[cfg(unix)]
    {
        let git_dir = live_root.join(".git");
        let dir = std::fs::File::open(&git_dir)
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
        dir.sync_all()
            .map_err(|e| GitFinalizeError::FinalizeFailed(crate::Error::Io(e)))?;
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
        let plan = GitFinalizePlan {
            repo_create: false,
            new_index_sha256: Some(sha256_bytes(
                &fs::read(live.join(".git").join("index")).unwrap(),
            )),
            ref_plans: Vec::new(),
            repo_create_owner: None,
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
        let result = install_index_with_lock(&live, &repo, &staging_repo, new_oid, &snapshot);
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
}
