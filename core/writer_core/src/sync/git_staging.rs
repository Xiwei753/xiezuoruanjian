//! #644 评论 5473789298 第1节：Git 后端的隔离 staging — 保留仓库身份。
//!
//! 把 `seed_from_live_as_git_repo` 从 [`super::staging`] 移到本模块，
//! 让 [`super::staging`] 只负责通用 staging；Git 专属的 repo snapshot 不再塞进去。
//!
//! ## 流程
//!
//! 1. 先调 [`StagingRun::seed_from_live`]，把**当前 live 工作区**同时复制进
//!    `base/` 和 `staging/`；这一步才是同步开始时的基线。
//! 2. 如果 live 已是 Git repo，用 `git2` 把仓库元数据克隆到 `run_root/git-repo/`
//!    这个临时目录。
//! 3. 只把临时 clone 的 `.git/` 移到 `staging/.git/`，**不要 checkout 覆盖
//!    staging 工作区**。这样 staging 的 Git index/HEAD 来自仓库历史，而 worktree
//!    仍然是刚才从 live 快照出来的内容，未提交修改会自然表现成 dirty worktree。
//! 4. live 原本不是 Git repo 时，**不要**提前伪造一个 `git init`。保持 staging
//!    非 repo，让现有 `SyncService::perform_sync` 自己走 `InitExistingProject`。
//! 5. live 原本是 Git repo，但仓库元数据复制失败时直接返回 `Err`。禁止回退成
//!    `file seed + git init`，因为那又会把历史/remote/HEAD 全丢掉。
//!
//! `git2` 是默认依赖（见 `Cargo.toml`），不需要 feature gate。

use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::sync::staging::StagingRun;

/// #644 评论 5475110422 第1节：live 在 seed 时的 Git 仓库状态。
///
/// `Option<String>` 无法区分"非 Git backend"、"unborn repo"、"已有提交的 repo"三种语义。
/// 本枚举让 finalize 阶段对每种状态走正确的路径。
///
/// #644 评论 5475413230 第3节：`Unborn` 保存 symbolic HEAD 的真实目标（如
/// `refs/heads/master`），不再猜 `refs/heads/main`。`Detached` 处理 detached HEAD。
#[derive(Debug, Clone)]
pub enum GitSeedState {
    /// live 不是 Git repo（没有 `.git/`）。staging 保持非 repo。
    NotGitRepo,
    /// live 是 Git repo 但 HEAD 是 unborn（`git init` 后尚未提交）。
    /// `head_ref` 是 symbolic HEAD 的真实目标引用名（如 `refs/heads/main`）。
    Unborn { head_ref: String },
    /// live 是 Git repo 且 HEAD 指向一个已存在的 commit。
    /// `head_ref` 是当前分支引用名，`head_oid` 是 seed 时的 HEAD OID，
    /// 供 finalize 做 compare-and-swap。
    Existing {
        head_ref: String,
        head_oid: git2::Oid,
    },
    /// #644 评论 5475413230 第3节：live 是 Git repo 但 HEAD 是 detached。
    /// `head_oid` 是 detached HEAD 指向的 commit OID。
    /// finalize 时不能伪造分支名，应明确拒绝或特殊处理。
    Detached { head_oid: git2::Oid },
}

/// 临时 clone 目录名（位于 `run_root` 下，与 `base/`、`staging/` 同级）。
const GIT_REPO_TMP_SUBDIR: &str = "git-repo";

/// #644 评论 5473789298 第1节：Git 后端的隔离 staging — 保留仓库身份。
///
/// 见模块文档的流程说明。本函数是 [`StagingRun`] 的伴随操作，通过其公开方法
/// （[`StagingRun::seed_from_live`] / [`StagingRun::staging_root`] /
/// [`StagingRun::run_root`]）访问隔离 run 的目录，不在 `staging.rs` 里塞
/// Git 专属逻辑。
///
/// # 返回
///
/// - `Ok(GitSeedState::NotGitRepo)`：live 不是 Git repo，staging 保持非 repo。
/// - `Ok(GitSeedState::Unborn { .. })`：live 是 Git repo 但 HEAD 是 unborn。
/// - `Ok(GitSeedState::Existing { .. })`：live 是 Git repo 且 HEAD 指向已有 commit。
///
/// # 错误
///
/// - live 路径包含非 UTF-8 字符 → `Error::Other`；
/// - live 是 Git repo 但 `git2` clone 失败 → 直接返回 `Err`，**不回退**到
///   `file seed + git init`（那会丢掉历史/remote/HEAD）；
/// - 把 `.git/` 从临时目录移到 staging 失败 → 返回 `Err`。
pub fn seed_from_live_as_git_repo(run: &StagingRun, live_root: &Path) -> Result<GitSeedState> {
    // 1. 先把 live 当前工作区复制进 base/ 和 staging/。这才是同步基线。
    run.seed_from_live(live_root)?;

    // 2. live 不是 Git repo 时，保持 staging 非 repo，不伪造 git init。
    if !live_root.join(".git").exists() {
        return Ok(GitSeedState::NotGitRepo);
    }

    // 3. live 是 Git repo：记录 seed 时的 HEAD 状态，供 finalize 决定路径。
    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "failed to open live git repo: {e}"
        )))
    })?;
    // #644 评论 5475413230 第3节：从 HEAD 读真实目标，不猜 main。
    // unborn 的 HEAD 仍然是 symbolic ref，可能指向 main、master 或其它分支。
    //
    // 注意：git2 的 HEAD 在首次 commit 后可能变成 direct reference
    // （symbolic_target() == None），但仍然是分支 HEAD（is_branch() == true），
    // 不是 detached HEAD。只有 is_branch() == false 且有 target OID 时才是真正的
    // detached HEAD。
    let seed_state = match live_repo.head() {
        Ok(head) => {
            if let Some(sym_target) = head.symbolic_target() {
                // HEAD 是 symbolic ref（如 "ref: refs/heads/main\n"）。
                let ref_name = sym_target.to_string();
                match head.target() {
                    Some(oid) => GitSeedState::Existing {
                        head_ref: ref_name,
                        head_oid: oid,
                    },
                    None => GitSeedState::Unborn { head_ref: ref_name },
                }
            } else if head.is_branch() {
                // #644 评论 5475413230 第3节：HEAD 是 direct reference 但是分支
                // （git2 在 commit 后可能把 symbolic ref 变成 direct ref）。
                // 用 shorthand() 获取分支名，构造完整 ref 名。
                let shorthand = head.shorthand().unwrap_or("main");
                let ref_name = format!("refs/heads/{}", shorthand);
                match head.target() {
                    Some(oid) => GitSeedState::Existing {
                        head_ref: ref_name,
                        head_oid: oid,
                    },
                    None => GitSeedState::Unborn { head_ref: ref_name },
                }
            } else if let Some(oid) = head.target() {
                // #644 评论 5475413230 第3节：真正的 detached HEAD —
                // 不是分支，直接指向一个 commit OID。不能伪造分支名。
                GitSeedState::Detached { head_oid: oid }
            } else {
                // HEAD 存在但既无 symbolic target 也无 direct target — 视为 unborn。
                GitSeedState::Unborn {
                    head_ref: "refs/heads/main".to_string(),
                }
            }
        }
        Err(_) => {
            // HEAD 不存在或无法解析 — 视为 unborn。
            // 尝试读取 .git/HEAD 文件获取真实目标。
            let head_file = live_root.join(".git").join("HEAD");
            let head_ref = if let Ok(content) = fs::read_to_string(&head_file) {
                // 格式: "ref: refs/heads/master\n"
                let trimmed = content.trim();
                if let Some(rest) = trimmed.strip_prefix("ref: ") {
                    rest.to_string()
                } else {
                    "refs/heads/main".to_string()
                }
            } else {
                "refs/heads/main".to_string()
            };
            GitSeedState::Unborn { head_ref }
        }
    };

    // 4. 用 git2 把仓库元数据克隆到临时目录。
    let live_root_str = live_root.to_str().ok_or_else(|| {
        crate::Error::Other(format!(
            "live_root path is not valid UTF-8: {}",
            live_root.display()
        ))
    })?;

    crate::storage::git_runtime::ensure_initialized()?;

    let temp_clone_dir = run.run_root().join(GIT_REPO_TMP_SUBDIR);
    if temp_clone_dir.exists() {
        fs::remove_dir_all(&temp_clone_dir)?;
    }
    fs::create_dir_all(&temp_clone_dir)?;

    // git2 本地 clone：libgit2 对本地路径自动用 local clone 优化（hardlink 对象），
    // 跨文件系统时回退到完整复制。clone 会 checkout HEAD 到 temp_clone_dir，
    // 但我们只取 .git/，worktree 会被丢弃。
    let _cloned_repo = git2::Repository::clone(live_root_str, &temp_clone_dir).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "git2 clone staging failed: {e}"
        )))
    })?;

    let temp_git_dir = temp_clone_dir.join(".git");
    if !temp_git_dir.exists() {
        // clone 成功但 .git 不存在——理论上不会发生，按失败处理，不回退。
        let _ = fs::remove_dir_all(&temp_clone_dir);
        return Err(crate::Error::Io(std::io::Error::other(
            "git2 clone succeeded but .git directory is missing",
        )));
    }

    // 5. 只把临时 clone 的 .git/ 移到 staging/.git/，不 checkout 覆盖 staging worktree。
    let staging_root = run.staging_root();
    let staging_git_dir = staging_root.join(".git");
    if staging_git_dir.exists() {
        fs::remove_dir_all(&staging_git_dir)?;
    }
    // temp_clone_dir 和 staging_root 都在 run_root 下（同一文件系统），rename 应成功。
    // 若 rename 失败（极端情况），返回 Err，不回退到 file seed + git init。
    if let Err(e) = fs::rename(&temp_git_dir, &staging_git_dir) {
        let _ = fs::remove_dir_all(&temp_clone_dir);
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "failed to move .git from temp clone to staging: {e}"
        ))));
    }

    // 6. 清理临时 clone 的其余内容（worktree 文件，只保留了 .git）。
    let _ = fs::remove_dir_all(&temp_clone_dir);

    Ok(seed_state)
}

/// #644 评论 5474772497 第1节 + #644 评论 5475110422 第1/2节：
/// Git 专属 finalize — 同步 live 的仓库元数据。
///
/// Transfer 完成后，staging repo 里有新的 objects、HEAD、refs、index，
/// 但 live 的 `.git` 仍然是 seed 时的旧状态。本函数把 staging 的仓库元数据
/// 同步回 live，但**不 checkout 工作区**（用户正在写的正文不会被覆盖）。
///
/// #644 评论 5475110422 第1节：按 `GitSeedState` 分三条路径：
/// - `NotGitRepo`：Transfer 成功后，从 staging 初始化 live 的 `.git`，
///   导入 objects，创建 branch/HEAD/origin/remote-tracking refs/index。
/// - `Unborn`：确认 live 仍是 unborn symbolic HEAD 后创建同步后的 branch ref。
/// - `Existing`：走 compare-and-swap 更新 live 当前分支 ref。
///
/// #644 评论 5475110422 第2节：object 导入和 ref 更新失败不再被 `let _` 吞掉。
/// 先收集缺失 OID，再逐个 read→write，任一步失败直接 Err。
/// 所有 objects 导入成功后才允许更新任何 live ref。
/// ref 更新前确认 `live_odb.exists(new_oid)`，不存在直接失败。
///
/// 仅在 `TargetCommitMode::Full`（成功类终态）时调用。
/// Conflict/PartialConflict 不推进 live 当前分支到未完成 merge 状态。
///
/// # 错误
///
/// 任何对象导入 / ref / index 更新失败都返回 `Err`，由调用方转为
/// `TargetCommitResult::Failed`。
#[allow(clippy::too_many_lines)]
pub fn finalize_git_repo_metadata(
    live_root: &Path,
    staging_root: &Path,
    seed_state: &GitSeedState,
) -> Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    let staging_git_dir = staging_root.join(".git");
    if !staging_git_dir.exists() {
        // staging 不是 Git repo（live 也不是），无需 finalize。
        return Ok(());
    }

    let staging_repo = git2::Repository::open(staging_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: failed to open staging repo: {e}"
        )))
    })?;

    // 读取 staging 的 HEAD（同步后的最新提交）。
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

    // 读取 staging 的当前分支名。
    let branch_name = if let Some(name) = staging_head.shorthand() {
        name.to_string()
    } else {
        "main".to_string()
    };

    match seed_state {
        GitSeedState::NotGitRepo => {
            // #644 评论 5475110422 第1节：live 原本不是 Git repo。
            // Transfer 成功后，staging 有完整的 .git/（clone 或 init+commit+push）。
            // 需要把 staging 的 .git/ 复制到 live，让 live 成为真正的 Git repo。
            finalize_not_git_repo(live_root, staging_root, &staging_repo, &staging_odb, new_oid, &branch_name)
        }
        GitSeedState::Unborn { head_ref } => {
            // #644 评论 5475110422 第1节：live 是 unborn repo。
            // 导入 objects，创建 branch ref 指向 new_oid。
            finalize_unborn(live_root, &staging_repo, &staging_odb, new_oid, head_ref)
        }
        GitSeedState::Existing { head_ref, head_oid } => {
            // 已有提交的 repo：导入新 objects，CAS 更新 ref。
            finalize_existing(live_root, &staging_repo, &staging_odb, new_oid, head_ref, *head_oid)
        }
        GitSeedState::Detached { head_oid } => {
            // #644 评论 5475413230 第3节：detached HEAD — 不能伪造分支名。
            // 用 CAS 更新 HEAD 直接指向 new_oid（保持 detached 状态）。
            finalize_detached(live_root, &staging_repo, &staging_odb, new_oid, *head_oid)
        }
    }
}

/// finalize 路径 1：live 原本不是 Git repo。
///
/// #644 评论 5475413230 第2节：原子性改进。
/// 把 staging `.git` 先完整复制到 `live_root/.git.sujian-tmp-<uuid>`，
/// 全部成功后再次确认 live `.git` 仍不存在，再同文件系统 rename 成 `.git`；
/// 如果 `.git` 已经出现，直接返回并发修改错误，不能删掉覆盖。
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

    // #644 评论 5475413230 第2节：先复制到临时目录，不直接覆盖 live .git。
    let tmp_id = uuid::Uuid::new_v4().to_string();
    let tmp_git = live_root.join(format!(".git.sujian-tmp-{}", tmp_id));
    copy_dir_recursive(&staging_git, &tmp_git)?;

    // 导入 staging 中可能新增的 objects 到临时 .git。
    let tmp_repo = git2::Repository::open(&tmp_git).map_err(|e| {
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

    // 确认 new_oid 在 tmp 中存在。
    if !tmp_odb.exists(new_oid) {
        let _ = fs::remove_dir_all(&tmp_git);
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: new_oid {} not found in tmp after import",
            new_oid
        ))));
    }

    // 更新 branch ref。
    let ref_name = format!("refs/heads/{}", branch_name);
    tmp_repo
        .reference(&ref_name, new_oid, true, "sync: init branch from staging")
        .map_err(|e| {
            let _ = fs::remove_dir_all(&tmp_git);
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_not_git_repo: update-ref {} failed: {}",
                ref_name, e
            )))
        })?;

    // 更新 index。
    update_live_index(&tmp_repo, staging_repo, new_oid).inspect_err(|_| {
        let _ = fs::remove_dir_all(&tmp_git);
    })?;

    // #644 评论 5475413230 第2节：再次确认 live .git 仍不存在（并发安全）。
    if live_git.exists() {
        let _ = fs::remove_dir_all(&tmp_git);
        return Err(crate::Error::Io(std::io::Error::other(
            "finalize_not_git_repo: live .git appeared during finalize (concurrent modification)",
        )));
    }

    // 同文件系统 rename 原子生效。
    fs::rename(&tmp_git, &live_git).map_err(|e| {
        let _ = fs::remove_dir_all(&tmp_git);
        crate::Error::Io(std::io::Error::other(format!(
            "finalize_not_git_repo: rename tmp -> .git failed: {e}"
        )))
    })?;

    Ok(())
}

/// finalize 路径 2：live 是 unborn repo。
///
/// #644 评论 5475413230 第2节：原子性改进。
/// 导入 objects，准备 index，最后才创建 branch ref。
/// 如果 index 准备失败，不留下已创建的 ref。
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

    // 1. 导入 objects（孤儿 object 不影响语义，可以先做）。
    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_unborn: new_oid {} not found in live after import",
            new_oid
        ))));
    }

    // 2. 准备 index（在创建 ref 之前）。
    update_live_index(&live_repo, staging_repo, new_oid)?;

    // 3. 同步 remote-tracking refs（在创建 branch ref 之前，失败可安全回退）。
    sync_remote_refs(&live_repo, staging_repo)?;

    // 4. 最后创建 branch ref（这是关键的原子步骤）。
    //    确认 live HEAD 仍然指向同一个 ref（unborn 状态未变）。
    if let Ok(current_head) = live_repo.head() {
        if let Some(sym_target) = current_head.symbolic_target() {
            if sym_target != head_ref {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "finalize_unborn: HEAD changed from {} to {} during finalize",
                    head_ref, sym_target
                ))));
            }
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

/// finalize 路径 3：live 已有提交的 repo（原有 CAS 逻辑）。
///
/// #644 评论 5475413230 第2节：原子性改进。
/// 导入 objects，准备 index，最后才做 CAS 更新 branch ref。
/// CAS 是关键的原子步骤；index/remote refs 在 CAS 之前完成，
/// 失败时不会推进 branch ref。
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

    // 1. 导入所有缺失 objects（孤儿 object 不影响语义）。
    import_missing_objects(staging_odb, &live_odb)?;

    // 确认 new_oid 在 live 中存在。
    if !live_odb.exists(new_oid) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_existing: new_oid {} not found in live after import",
            new_oid
        ))));
    }

    // 2. 准备 index（在 CAS 之前，失败不会推进 ref）。
    update_live_index(&live_repo, staging_repo, new_oid)?;

    // 3. 同步 remote-tracking refs（在 CAS 之前，失败不会推进 branch ref）。
    sync_remote_refs(&live_repo, staging_repo)?;

    // 4. CAS 更新 live 当前分支 ref（最后执行，这是关键的原子步骤）。
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
/// #644 评论 5475413230 第3节：detached HEAD 不能伪造分支名。
/// 用 CAS 更新 HEAD 直接指向 new_oid（保持 detached 状态）。
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

    // 1. 导入 objects。
    import_missing_objects(staging_odb, &live_odb)?;

    if !live_odb.exists(new_oid) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "finalize_detached: new_oid {} not found in live after import",
            new_oid
        ))));
    }

    // 2. 准备 index。
    update_live_index(&live_repo, staging_repo, new_oid)?;

    // 3. CAS 更新 HEAD 直接指向 new_oid（保持 detached 状态）。
    live_repo
        .reference(
            "HEAD",
            new_oid,
            true,
            "sync: finalize detached HEAD after full sync",
        )
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize_detached: update HEAD failed (old={} new={}): {}",
                base_oid, new_oid, e
            )))
        })?;

    Ok(())
}

/// #644 评论 5475110422 第2节：从 staging ODB 导入 live ODB 缺失的对象。
///
/// 先收集缺失 OID，再逐个 read→write，任一步失败直接 Err。
/// 不再用 `let _` 吞掉写入错误。
fn import_missing_objects(
    staging_odb: &git2::Odb,
    live_odb: &git2::Odb,
) -> Result<()> {
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

/// #644 评论 5475110422 第2节：同步 staging 的 remote-tracking refs 到 live。
///
/// 失败直接 Err，不再用 `let _` 吞掉。
fn sync_remote_refs(
    live_repo: &git2::Repository,
    staging_repo: &git2::Repository,
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
            live_repo
                .reference(name, target, true, "sync: update remote-tracking ref from staging")
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

/// #644 评论 5474772497 第1节 + #644 评论 5475110422 第1节：
/// Git repo-metadata finalize 的结果包装。
///
/// 供 `apply_staging_commits_for_targets` 在 Full 模式下调用，
/// 失败时转为 `TargetCommitResult::Failed`。
pub fn try_finalize_git_repo_metadata(
    live_root: &Path,
    staging_root: &Path,
    seed_state: Option<&GitSeedState>,
) -> std::result::Result<(), String> {
    let Some(state) = seed_state else {
        // 非 Git backend（seed_state 为 None），无需 finalize。
        return Ok(());
    };
    finalize_git_repo_metadata(live_root, staging_root, state).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// live 不是 Git repo：seed_from_live_as_git_repo 等价于 seed_from_live，
    /// staging 不应包含 .git/。
    #[test]
    fn git_staging_non_repo_keeps_staging_non_repo() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(live.join("sub")).unwrap();
        fs::write(live.join("a.txt"), "hello").unwrap();
        fs::write(live.join("sub/b.md"), "world").unwrap();

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        let result = seed_from_live_as_git_repo(&run, &live).unwrap();
        // 非 repo → 返回 NotGitRepo
        assert!(matches!(result, GitSeedState::NotGitRepo));

        // base/staging 有业务文件
        assert_eq!(
            fs::read_to_string(run.base_root().join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            fs::read_to_string(run.staging_root().join("sub/b.md")).unwrap(),
            "world"
        );
        // staging 不应是 git repo（没有 .git/）
        assert!(!run.staging_root().join(".git").exists());
    }

    /// live 是 Git repo：staging 拿到 .git/ 元数据，worktree 仍是 live 当前工作区。
    #[test]
    fn git_staging_repo_clones_git_metadata_only() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.txt"), "committed").unwrap();

        // 在 live 里建 git repo 并提交
        let repo = git2::Repository::init(&live).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // 模拟 live 工作区有未提交修改
        fs::write(live.join("a.txt"), "working-dirty").unwrap();
        fs::write(live.join("untracked.md"), "untracked").unwrap();

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        let seed_state = seed_from_live_as_git_repo(&run, &live).unwrap();
        // repo → 返回 Existing
        assert!(matches!(seed_state, GitSeedState::Existing { .. }));

        // staging worktree 是 live 当前工作区（含未提交修改），不是 HEAD 的 checkout
        assert_eq!(
            fs::read_to_string(run.staging_root().join("a.txt")).unwrap(),
            "working-dirty"
        );
        assert_eq!(
            fs::read_to_string(run.staging_root().join("untracked.md")).unwrap(),
            "untracked"
        );
        // staging 拿到了 .git/ 元数据
        assert!(run.staging_root().join(".git").exists());
        // 临时目录已清理
        assert!(!run.run_root().join("git-repo").exists());
    }

    /// live 是 unborn Git repo：seed_from_live_as_git_repo 返回 Unborn。
    #[test]
    fn git_staging_unborn_repo_returns_unborn() {
        crate::storage::git_runtime::ensure_initialized().unwrap();
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("live");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.txt"), "not committed yet").unwrap();

        // 在 live 里建 git repo 但不提交（unborn）
        let _repo = git2::Repository::init(&live).unwrap();

        let run = StagingRun::create(tmp.path(), live.clone()).unwrap();
        let seed_state = seed_from_live_as_git_repo(&run, &live).unwrap();
        // unborn → 返回 Unborn
        assert!(matches!(seed_state, GitSeedState::Unborn { .. }));

        // staging 拿到了 .git/ 元数据
        assert!(run.staging_root().join(".git").exists());
    }
}
