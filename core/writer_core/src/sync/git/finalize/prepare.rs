use std::fs;
use std::path::Path;

use super::super::model::*;
use super::super::seed::GitSeedState;
use super::temp::*;
use crate::error::Result;

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

// ── 内部准备辅助 ──

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
pub(crate) fn open_live_repo(
    live_root: &Path,
    explicit_git_dir: Option<&Path>,
) -> Result<git2::Repository> {
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
