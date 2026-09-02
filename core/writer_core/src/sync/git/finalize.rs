use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;
use super::seed::GitSeedState;

use super::model::*;
use super::locks::*;

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
    explicit_git_dir: Option<&Path>,
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

    // #644 评论 5492740265 问题3：用 open_live_repo 统一入口打开 live repo。
    // 外部 git_dir 布局下 worktree 没有 .git，Repository::open(live_root) 会失败。
    let live_repo = open_live_repo(live_root, explicit_git_dir)?;

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
    let index_path = live_repo.path().join("index");
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
#[allow(clippy::excessive_nesting, clippy::too_many_lines)]
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
            ref_tx_owner: None,
            ref_lock_names: Vec::new(),
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

    // #644 评论 5489192105 问题2：ref_plans 非空时生成 ref_tx_owner uuid，
    // 供 RefTransaction 做持久 ownership。空 ref_plans 时为 None。
    let ref_tx_owner = if !ref_plans.is_empty() {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        None
    };

    // #644 评论 5490206957 问题3：构建完整的 ref_lock_names 集合。
    // 不要从 ref_plans 反推会锁哪些 refs——Unborn/Existing 的 ref_plans 只有
    // head_ref，但 forward 实际还锁了 HEAD + remote refs。
    // 这里一次写全：HEAD + head_ref + remote refs。
    let mut ref_lock_names: Vec<String> = Vec::new();
    match seed_state {
        GitSeedState::NotGitRepo => {
            // NotGitRepo：forward 不锁 live refs（live 还没有 .git），
            // 但 rollback 仍可能锁 HEAD + branch + remote refs。
            // ref_lock_names 为空即可，因为 NotGitRepo 的 forward 不经过 RefTransaction。
        }
        GitSeedState::Unborn { head_ref } | GitSeedState::Existing { head_ref, .. } => {
            ref_lock_names.push("HEAD".to_string());
            ref_lock_names.push(head_ref.clone());
        }
        GitSeedState::Detached { .. } => {
            ref_lock_names.push("HEAD".to_string());
        }
    }
    for (name, _, _) in &ref_plans {
        if !ref_lock_names.contains(name) {
            ref_lock_names.push(name.clone());
        }
    }
    ref_lock_names.sort();
    ref_lock_names.dedup();

    Ok(GitFinalizePlan {
        repo_create: matches!(seed_state, GitSeedState::NotGitRepo),
        new_index_sha256: staging_plan.new_index_sha256,
        ref_plans,
        repo_create_owner: new_repo_create_owner(seed_state),
        index_lock_owner,
        ref_tx_owner,
        ref_lock_names,
    })
}

/// #644 评论 5481496190 问题3：NotGitRepo 路径生成 owner marker uuid。
/// 非 NotGitRepo 路径返回 None。
pub(crate) fn new_repo_create_owner(seed_state: &GitSeedState) -> Option<String> {
    if matches!(seed_state, GitSeedState::NotGitRepo) {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        None
    }
}

/// 从 git2 Reference 构造 RefSnapshot。
pub(crate) fn snapshot_ref_from_repo_ref(reference: &git2::Reference<'_>) -> RefSnapshot {
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
pub(crate) fn snapshot_ref(
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

/// #644 评论 5492740265 问题3：打开 live Git 仓库的唯一入口。
///
/// 统一构造 `GitRepoLayout`（标准布局或外部 git_dir），调用
/// `crate::storage::git_repo_layout::open_repo_with_layout()`。
///
/// 外部 git_dir 布局下共享 worktree 没有 `.git`，`Repository::open(live_root)` 会失败
///（worktree 没有 .git/gitlink）。本函数始终从真实 git_dir 打开仓库并设置正确的
/// workdir，标准布局和外部 git_dir 都走这一套。
///
/// - `explicit_git_dir = Some(p)`：外部 git_dir 布局，从 `p` 打开，workdir 指向 `live_root`。
/// - `explicit_git_dir = None`：标准布局，等效于 `Repository::open(live_root)`。
pub(crate) fn open_live_repo(live_root: &Path, explicit_git_dir: Option<&Path>) -> Result<git2::Repository> {
    let layout = match explicit_git_dir {
        Some(git_dir) => crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
            live_root.to_path_buf(),
            git_dir.to_path_buf(),
        ),
        None => crate::storage::git_repo_layout::GitRepoLayout::new(live_root.to_path_buf()),
    };
    crate::storage::git_repo_layout::open_repo_with_layout(&layout).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "open_live_repo: failed to open live repo (live_root={}, git_dir={}): {e}",
            live_root.display(),
            layout.git_dir.display(),
        )))
    })
}

/// #644 评论 5492740265 问题2：repo install tmp 路径的统一计算。
///
/// tmp 永远建在**最终 live_git 的父目录**，保证 `tmp -> live_git` 是同一文件系统的
/// 原子 rename。Android 上 live_root（共享 worktree）和 live_git（私有 git_dir）
/// 在不同文件系统，把 tmp 建在 live_root 会导致 rename 跨 mount 失败。
///
/// - `explicit_git_dir = Some(p)`：tmp 建在 `p.parent().join(".git.sujian-tmp-{owner}")`。
/// - `explicit_git_dir = None`：tmp 建在 `live_root.join(".git.sujian-tmp-{owner}")`
///   （标准布局，live_git = live_root/.git，父目录就是 live_root）。
///
/// forward（`finalize_not_git_repo`）和 recovery（`rollback_git_finalize`）必须对同一
/// owner 算出同一物理 tmp 路径，否则崩溃后找不到自己留下的 repo。
pub(crate) fn repo_install_tmp_path(
    live_root: &Path,
    explicit_git_dir: Option<&Path>,
    owner: &str,
) -> Result<PathBuf> {
    match explicit_git_dir {
        Some(git_dir) => {
            let parent = git_dir.parent().ok_or_else(|| {
                crate::Error::Io(std::io::Error::other(format!(
                    "repo_install_tmp_path: explicit_git_dir has no parent: {}",
                    git_dir.display(),
                )))
            })?;
            Ok(parent.join(format!(".git.sujian-tmp-{}", owner)))
        }
        None => Ok(live_root.join(format!(".git.sujian-tmp-{}", owner))),
    }
}

/// #644 评论 5475805198 第2节：应用 Git metadata 变更到 live。
///
/// 在 `SaveTransaction.commit()` 成功后调用。
/// 成功后调用方应调用 `SaveTransaction::finish()` 清理事务。
///
/// #644 评论 5488871385 问题1：本函数**不再内部 rollback**。
/// 失败时只返回错误，真正的 rollback 由统一 coordinator 负责：
/// `inspect_git_rollback_state()` → `preflight_backup_entries()` →
/// `rollback_git_finalize()` → file rollback。
/// 这样 backup preflight 一定在 Git rollback 之前完成。
///
/// #644 评论 5477439446 问题2：`ConcurrentMetadataChanged` 表示本轮尚未修改
/// live Git metadata，调用方不应调用 `rollback_git_finalize()`。
///
/// #644 评论 5480360027：接收 `plan`（write-ahead plan），不再维护内存 mutation_log。
/// finalize 成功就 finish，失败/崩溃恢复都只依赖磁盘上的 plan。
pub fn commit_git_finalize(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    // 调用内部 finalize。失败时直接返回错误，不内部 rollback。
    // 调用方（sync_ops.rs）负责 inspect → preflight → rollback → file rollback。
    finalize_git_repo_metadata_inner(
        live_root,
        staging_root,
        seed_state,
        snapshot,
        plan,
        explicit_git_dir,
    )
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
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    // 尝试完成 Git finalize。
    match finalize_git_repo_metadata_inner(
        live_root,
        staging_root,
        seed_state,
        snapshot,
        plan,
        explicit_git_dir,
    ) {
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
                match super::rollback::rollback_git_finalize(live_root, snapshot, plan, None) {
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
    explicit_git_dir: Option<&Path>,
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
    commit_git_finalize(live_root, staging_root, state, snap, plan, explicit_git_dir)
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
pub fn cleanup_repo_create_owner_marker(
    live_root: &Path,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) {
    if plan.repo_create_owner.is_some() {
        let default_git_dir = live_root.join(".git");
        let git_dir_ref = explicit_git_dir.unwrap_or(&default_git_dir);
        let marker_path = git_dir_ref.join(".sujian-sync-owner");
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
    explicit_git_dir: Option<&Path>,
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
            explicit_git_dir,
        ),
        GitSeedState::Unborn { head_ref } => finalize_unborn(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            head_ref,
            snapshot,
            plan,
            explicit_git_dir,
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
            explicit_git_dir,
        ),
        GitSeedState::Detached { head_oid } => finalize_detached(
            live_root,
            &staging_repo,
            &staging_odb,
            new_oid,
            *head_oid,
            snapshot,
            plan,
            explicit_git_dir,
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
#[allow(clippy::too_many_arguments)]
fn finalize_not_git_repo(
    live_root: &Path,
    staging_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    branch_name: &str,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    let staging_git = staging_root.join(".git");
    let live_git = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));

    // #644 评论 5482310913 问题2：tmp_git 目录名基于 repo_create_owner，
    // 使恢复时能精准清理本轮 tmp repo，不用扫猜。
    // owner 为 None 时（旧 plan 兼容）回退到随机 uuid。
    let tmp_id = plan
        .repo_create_owner
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    // #644 评论 5492740265 问题2：tmp 建在 live_git 父目录，
    // 保证 tmp -> live_git 同一文件系统原子 rename。
    // Android 上 live_root（共享 worktree）和 live_git（私有 git_dir）在不同文件系统，
    // 把 tmp 建在 live_root 会导致 rename 跨 mount 失败。
    let tmp_git = repo_install_tmp_path(live_root, explicit_git_dir, &tmp_id)
        .map_err(GitFinalizeError::FinalizeFailed)?;
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
#[allow(clippy::too_many_arguments)]
fn finalize_unborn(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    head_ref: &str,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    // #644 评论 5492740265 问题3：用 open_live_repo 统一入口，
    // 外部 git_dir 布局下 Repository::open(live_root) 会失败。
    let live_repo =
        open_live_repo(live_root, explicit_git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
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
    verify_git_metadata_unchanged(live_root, snapshot, head_ref, explicit_git_dir)?;

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
        explicit_git_dir,
    )?;

    // #644 评论 5490799656 问题4：统一使用 plan 作为唯一事实。
    // 不再调用 collect_remote_ref_actions 从 staging 重算第二份执行计划。
    // 直接用 plan.ref_lock_names 做 acquire，plan.ref_plans 做 CAS classify + 执行。
    {
        use super::tx::RefTransaction;

        let ref_tx_owner = plan.ref_tx_owner.as_deref().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_unborn: plan.ref_tx_owner is None but ref_plans is non-empty",
            )))
        })?;

        // 用 plan.ref_lock_names（完整的 forward lock 集合）做 acquire。
        let ref_names = &plan.ref_lock_names;

        let mut ref_tx = RefTransaction::acquire_all_refs(&live_repo, ref_names, ref_tx_owner)
            .map_err(GitFinalizeError::FinalizeFailed)?;

        // 锁内读取 HEAD，确认仍是 symbolic 且指向 head_ref。
        let raw_head = ref_tx.find_reference("HEAD").map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_unborn: HEAD reference not found: {e}"
            )))
        })?;
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

        // 锁内确认目标 branch ref 仍不存在（unborn 状态未变）。
        match ref_tx.find_reference(head_ref) {
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

        // 创建 branch ref（Unborn 的 ref_plans 中 head_ref 的 old=None）。
        ref_tx.set_target(head_ref, new_oid, "sync: create branch from staging")?;

        // #644 评论 5490799656 问题4：按 plan 执行剩余 refs（remote refs 等）。
        // Unborn 的 ref_plans 中 head_ref 已在上面处理，这里跳过它。
        execute_plan_refs_under_lock(&mut ref_tx, plan, Some(head_ref))?;

        // commit：提交所有 set_target，释放全部 lock + 删 owner marker。
        ref_tx.commit().map_err(GitFinalizeError::FinalizeFailed)?;
    }

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
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    // #644 评论 5492740265 问题3：用 open_live_repo 统一入口，
    // 外部 git_dir 布局下 Repository::open(live_root) 会失败。
    let live_repo =
        open_live_repo(live_root, explicit_git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
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
    verify_git_metadata_unchanged(live_root, snapshot, head_ref, explicit_git_dir)?;

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
        explicit_git_dir,
    )?;

    // #644 评论 5490799656 问题4：统一使用 plan 作为唯一事实。
    // 不再调用 collect_remote_ref_actions 从 staging 重算第二份执行计划。
    // 直接用 plan.ref_lock_names 做 acquire，plan.ref_plans 做 CAS classify + 执行。
    {
        use super::tx::RefTransaction;

        let ref_tx_owner = plan.ref_tx_owner.as_deref().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_existing: plan.ref_tx_owner is None but ref_plans is non-empty",
            )))
        })?;

        // 用 plan.ref_lock_names（完整的 forward lock 集合）做 acquire。
        let ref_names = &plan.ref_lock_names;

        let mut ref_tx = RefTransaction::acquire_all_refs(&live_repo, ref_names, ref_tx_owner)
            .map_err(GitFinalizeError::FinalizeFailed)?;

        // 锁内 verify：HEAD 仍指向 head_ref（用户未切 branch/detach）。
        let raw_head = ref_tx.find_reference("HEAD").map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: HEAD reference not found: {e}"
            ))))
        })?;
        match raw_head.symbolic_target() {
            Some(sym_target) if sym_target == head_ref => {
                // HEAD 仍 symbolic 且 target == head_ref：允许继续。
            }
            Some(other) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "finalize_existing: HEAD now points to {} but seed was {}",
                        other, head_ref
                    )),
                )));
            }
            None => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(
                        "finalize_existing: HEAD detached after preflight: index/refs \
                         may already be written, must rollback (refusing to advance \
                         head_ref on a branch the user has left via detach)",
                    ),
                )));
            }
        }
        drop(raw_head);

        // 锁内 verify branch ref 仍等于 base_oid（CAS 条件）。
        let branch_ref = ref_tx.find_reference(head_ref).map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: branch ref {} not found: {e}",
                head_ref
            ))))
        })?;
        let current_branch_oid = branch_ref.target().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_existing: branch ref {} has no target",
                head_ref
            ))))
        })?;
        if current_branch_oid != base_oid {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_existing: branch {} changed from {} to {} (concurrent modification)",
                    head_ref, base_oid, current_branch_oid
                )),
            )));
        }
        drop(branch_ref);

        // 锁内更新 branch ref（head_ref 的 CAS）。
        ref_tx.set_target(
            head_ref,
            new_oid,
            "sync: finalize git repo metadata after full sync",
        )?;

        // #644 评论 5490799656 问题4：按 plan 执行剩余 refs（remote refs 等）。
        // head_ref 已在上面处理，这里跳过它。
        execute_plan_refs_under_lock(&mut ref_tx, plan, Some(head_ref))?;

        // commit：提交所有 set_target，释放全部 lock + 删 owner marker。
        ref_tx.commit().map_err(GitFinalizeError::FinalizeFailed)?;
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
#[allow(clippy::too_many_arguments)]
fn finalize_detached(
    live_root: &Path,
    staging_repo: &git2::Repository,
    staging_odb: &git2::Odb,
    new_oid: git2::Oid,
    base_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    plan: &GitFinalizePlan,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    // #644 评论 5492740265 问题3：用 open_live_repo 统一入口，
    // 外部 git_dir 布局下 Repository::open(live_root) 会失败。
    let live_repo =
        open_live_repo(live_root, explicit_git_dir).map_err(GitFinalizeError::FinalizeFailed)?;
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
    verify_git_metadata_unchanged(live_root, snapshot, "HEAD", explicit_git_dir)?;

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
        explicit_git_dir,
    )?;

    // #644 评论 5490799656 问题4：统一使用 plan 作为唯一事实。
    // 不再调用 collect_remote_ref_actions 从 staging 重算第二份执行计划。
    // 直接用 plan.ref_lock_names 做 acquire，plan.ref_plans 做 CAS classify + 执行。
    {
        use super::tx::RefTransaction;

        let ref_tx_owner = plan.ref_tx_owner.as_deref().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_detached: plan.ref_tx_owner is None but ref_plans is non-empty",
            )))
        })?;

        // 用 plan.ref_lock_names（完整的 forward lock 集合）做 acquire。
        let ref_names = &plan.ref_lock_names;

        let mut ref_tx = RefTransaction::acquire_all_refs(&live_repo, ref_names, ref_tx_owner)
            .map_err(GitFinalizeError::FinalizeFailed)?;

        // 锁内 verify HEAD 仍 detached（未 resolve 的 HEAD 的 target 是 raw OID）。
        let raw_head = ref_tx.find_reference("HEAD").map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "finalize_detached: HEAD reference not found: {e}"
            ))))
        })?;
        if raw_head.symbolic_target().is_some() {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(
                    "finalize_detached: HEAD is now symbolic (concurrent modification)",
                ),
            )));
        }
        let current_head_oid = raw_head.target().ok_or_else(|| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                "finalize_detached: HEAD has no target",
            )))
        })?;
        if current_head_oid != base_oid {
            return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                std::io::Error::other(format!(
                    "finalize_detached: HEAD changed from {} to {} (concurrent modification)",
                    base_oid, current_head_oid
                )),
            )));
        }
        drop(raw_head);

        // 锁内更新 HEAD（CAS：current==base_oid）。
        ref_tx.set_target("HEAD", new_oid, "sync: finalize detached HEAD")?;

        // #644 评论 5490799656 问题4：按 plan 执行剩余 refs（remote refs 等）。
        // Detached 的 ref_plans 中 HEAD 已在上面处理，这里跳过它。
        execute_plan_refs_under_lock(&mut ref_tx, plan, Some("HEAD"))?;

        ref_tx.commit().map_err(GitFinalizeError::FinalizeFailed)?;
    }

    Ok(())
}

// ── 内部辅助函数（从 git_staging.rs 移入） ──

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
pub(crate) fn install_index_with_lock(
    live_root: &Path,
    _live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
    new_oid: git2::Oid,
    snapshot: &GitMetadataSnapshot,
    owner: &str,
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    // 1. 在 staging .git 目录下生成目标 index 字节。
    let target_index_bytes = generate_target_index_bytes(staging_repo, new_oid)?;

    let git_dir = explicit_git_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| live_root.join(".git"));
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
    explicit_git_dir: Option<&Path>,
) -> std::result::Result<(), GitFinalizeError> {
    // #644 评论 5492740265 问题3：用 open_live_repo 统一入口，
    // 外部 git_dir 布局下 Repository::open(live_root) 会失败。
    let live_repo =
        open_live_repo(live_root, explicit_git_dir).map_err(GitFinalizeError::FinalizeFailed)?;

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
    let index_path = live_repo.path().join("index");
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

/// #644 评论 5490799656 问题4：在 RefTransaction 锁保护下，按 write-ahead plan
/// 执行所有 ref 写入。
///
/// 不再从 staging 重算第二份执行计划（`collect_remote_ref_actions`）。
/// plan 是唯一事实：锁内读取所有 `plan.ref_plans`，每个 ref 只能是
/// old / new 两种本事务允许状态：
/// - 第三状态 → 立即退出，不写 index/ref（返回 `ConcurrentMetadataChanged`）；
/// - old → old→new action（set_target 或 create）；
/// - 已经是 new → replay no-op。
///
/// `exclude_ref` 用于排除已经在上层单独处理的 ref（如 finalize_existing 中的
/// head_ref CAS 已经在 classify 阶段处理）。
fn execute_plan_refs_under_lock(
    ref_tx: &mut super::tx::RefTransaction<'_>,
    plan: &GitFinalizePlan,
    exclude_ref: Option<&str>,
) -> std::result::Result<(), GitFinalizeError> {
    for (ref_name, old_oid_str, new_oid_str) in &plan.ref_plans {
        // 跳过已在上层处理的 ref。
        if Some(ref_name.as_str()) == exclude_ref {
            continue;
        }

        let new_oid = git2::Oid::from_str(new_oid_str).map_err(|e| {
            GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(format!(
                "execute_plan_refs_under_lock: invalid new_oid for {}: {e}",
                ref_name
            ))))
        })?;

        let current = ref_tx.find_reference(ref_name);

        match (old_oid_str, current) {
            // ref(old=None): 本事务新建 ref。
            // - Absent → create (set_target with force=true)
            // - current == new → replay no-op
            // - current == 其它 → third state, exit
            (None, Ok(current_ref)) => {
                if current_ref.target() == Some(new_oid) {
                    // replay no-op
                } else {
                    return Err(GitFinalizeError::ConcurrentMetadataChanged {
                        reason: format!(
                            "execute_plan_refs_under_lock: ref {} has unexpected value \
                             (expected absent or new_oid {}) — concurrent modification",
                            ref_name, new_oid
                        ),
                    });
                }
            }
            (None, Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                // Absent → create
                ref_tx.set_target(ref_name, new_oid, "sync: finalize plan ref (create)")?;
            }
            (None, Err(e)) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "execute_plan_refs_under_lock: lookup {}: {e}",
                        ref_name
                    )),
                )));
            }

            // ref(old=Some): 本事务更新 ref。
            // - current == old → CAS: set_target to new
            // - current == new → replay no-op
            // - current == 其它 → third state, exit
            (Some(old_oid_str), Ok(current_ref)) => {
                let old_oid = git2::Oid::from_str(old_oid_str).map_err(|e| {
                    GitFinalizeError::FinalizeFailed(crate::Error::Io(std::io::Error::other(
                        format!(
                            "execute_plan_refs_under_lock: invalid old_oid for {}: {e}",
                            ref_name
                        ),
                    )))
                })?;
                if current_ref.target() == Some(old_oid) {
                    // CAS: old → new
                    ref_tx.set_target(ref_name, new_oid, "sync: finalize plan ref (update)")?;
                } else if current_ref.target() == Some(new_oid) {
                    // replay no-op
                } else {
                    return Err(GitFinalizeError::ConcurrentMetadataChanged {
                        reason: format!(
                            "execute_plan_refs_under_lock: ref {} changed from {} to unexpected \
                             value (expected old_oid {} or new_oid {})",
                            ref_name,
                            current_ref
                                .target()
                                .map_or_else(|| "none".to_string(), |o| o.to_string()),
                            old_oid,
                            new_oid
                        ),
                    });
                }
            }
            (Some(_), Err(e)) if e.code() == git2::ErrorCode::NotFound => {
                return Err(GitFinalizeError::ConcurrentMetadataChanged {
                    reason: format!(
                        "execute_plan_refs_under_lock: ref {} not found (expected old or new)",
                        ref_name
                    ),
                });
            }
            (Some(_), Err(e)) => {
                return Err(GitFinalizeError::FinalizeFailed(crate::Error::Io(
                    std::io::Error::other(format!(
                        "execute_plan_refs_under_lock: lookup {}: {e}",
                        ref_name
                    )),
                )));
            }
        }
    }
    Ok(())
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
