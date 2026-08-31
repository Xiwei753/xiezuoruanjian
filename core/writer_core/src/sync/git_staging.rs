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
/// - `Ok(None)`：live 不是 Git repo，staging 保持非 repo。
/// - `Ok(Some(oid_hex))`：live 是 Git repo，seed 成功，返回 live 当前 HEAD 的
///   hex OID（供 [`finalize_git_repo_metadata`] 做 compare-and-swap）。
///
/// # 错误
///
/// - live 路径包含非 UTF-8 字符 → `Error::Other`；
/// - live 是 Git repo 但 `git2` clone 失败 → 直接返回 `Err`，**不回退**到
///   `file seed + git init`（那会丢掉历史/remote/HEAD）；
/// - 把 `.git/` 从临时目录移到 staging 失败 → 返回 `Err`。
pub fn seed_from_live_as_git_repo(run: &StagingRun, live_root: &Path) -> Result<Option<String>> {
    // 1. 先把 live 当前工作区复制进 base/ 和 staging/。这才是同步基线。
    run.seed_from_live(live_root)?;

    // 2. live 不是 Git repo 时，保持 staging 非 repo，不伪造 git init。
    if !live_root.join(".git").exists() {
        return Ok(None);
    }

    // 3. live 是 Git repo：记录 seed 时的 HEAD OID，供 finalize 做 CAS。
    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "failed to open live git repo: {e}"
        )))
    })?;
    let base_head_oid = live_repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string());

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

    Ok(base_head_oid)
}

/// #644 评论 5474772497 第1节：Git 专属 finalize — 同步 live 的仓库元数据。
///
/// Transfer 完成后，staging repo 里有新的 objects、HEAD、refs、index，
/// 但 live 的 `.git` 仍然是 seed 时的旧状态。本函数把 staging 的仓库元数据
/// 同步回 live，但**不 checkout 工作区**（用户正在写的正文不会被覆盖）。
///
/// 步骤：
/// 1. 从 staging 读取同步后的 HEAD、当前分支、remote refs；
/// 2. 把 staging 新产生且 live ODB 缺失的 objects 导入 live ODB；
/// 3. 用 compare-and-swap 语义更新 live 当前分支 ref
///    （`update_ref` 校验 old OID = seed 时的 base HEAD）；
/// 4. 把 live index 重建为同步后 HEAD tree（不 checkout 工作区）。
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
    base_head_oid_hex: &str,
) -> Result<()> {
    crate::storage::git_runtime::ensure_initialized()?;

    let staging_git_dir = staging_root.join(".git");
    if !staging_git_dir.exists() {
        // staging 不是 Git repo（live 也不是），无需 finalize。
        return Ok(());
    }

    let live_repo = git2::Repository::open(live_root).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: failed to open live repo: {e}"
        )))
    })?;
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

    // 1. 把 staging 新产生且 live ODB 缺失的 objects 导入 live ODB。
    let staging_odb = staging_repo.odb().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("finalize: staging odb: {e}")))
    })?;
    let live_odb = live_repo
        .odb()
        .map_err(|e| crate::Error::Io(std::io::Error::other(format!("finalize: live odb: {e}"))))?;

    staging_odb
        .foreach(|oid| {
            if live_odb.exists(*oid) {
                return true; // live 已有此对象，跳过。
            }
            // 从 staging 读取对象，写入 live ODB。
            if let Ok(obj) = staging_odb.read(*oid) {
                let _ = live_odb.write(obj.kind(), obj.data());
            }
            true
        })
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize: object import foreach: {e}"
            )))
        })?;

    // 2. 读取 staging 的当前分支名和 remote refs。
    let branch_name = if let Some(name) = staging_head.shorthand() {
        name.to_string()
    } else {
        // HEAD 是 detached — 用 "main" 兜底。
        "main".to_string()
    };

    // 3. 把 staging 的 remote-tracking refs 同步到 live。
    //    staging clone 自 live，Transfer 可能更新了 refs/remotes/origin/*。
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
            // 直接覆盖 live 的 remote-tracking ref（不需要 CAS）。
            let _ = live_repo.reference(
                name,
                target,
                true, // force
                "sync: update remote-tracking ref from staging",
            );
        }
    }

    // 4. 用 compare-and-swap 语义更新 live 当前分支 ref。
    //    只允许 old OID 仍等于 seed 时的 base HEAD（reference_matching 语义）。
    let base_oid = git2::Oid::from_str(base_head_oid_hex).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: invalid base_head_oid: {e}"
        )))
    })?;
    let ref_name = format!("refs/heads/{}", branch_name);
    live_repo
        .reference_matching(
            &ref_name,
            new_oid,
            false, // force=false
            base_oid,
            "sync: finalize git repo metadata after full sync",
        )
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "finalize: update-ref {} failed (CAS old={} new={}): {}",
                ref_name, base_oid, new_oid, e
            )))
        })?;

    // 5. 把 live index 重建为同步后 HEAD tree（不 checkout 工作区）。
    //    这样 live 的 index 与新 HEAD 一致，用户继续写的正文表现为 dirty worktree。
    let new_commit = staging_repo.find_commit(new_oid).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: find new commit: {e}"
        )))
    })?;
    let new_tree = new_commit.tree().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: find new tree: {e}"
        )))
    })?;

    let mut live_index = live_repo.index().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: get live index: {e}"
        )))
    })?;
    live_index.read_tree(&new_tree).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "finalize: index read_tree: {e}"
        )))
    })?;
    live_index.write().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!("finalize: index write: {e}")))
    })?;

    Ok(())
}

/// #644 评论 5474772497 第1节：Git repo-metadata finalize 的结果包装。
///
/// 供 `apply_staging_commits_for_targets` 在 Full 模式下调用，
/// 失败时转为 `TargetCommitResult::Failed`。
pub fn try_finalize_git_repo_metadata(
    live_root: &Path,
    staging_root: &Path,
    base_head_oid_hex: Option<&str>,
) -> std::result::Result<(), String> {
    let Some(oid_hex) = base_head_oid_hex else {
        // 非 Git backend（base_head_oid 为 None），无需 finalize。
        return Ok(());
    };
    finalize_git_repo_metadata(live_root, staging_root, oid_hex).map_err(|e| e.to_string())
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
        // 非 repo → 返回 None
        assert!(result.is_none());

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
        let base_oid = seed_from_live_as_git_repo(&run, &live).unwrap();
        // repo → 返回 Some(oid_hex)
        assert!(base_oid.is_some());
        assert!(!base_oid.as_ref().unwrap().is_empty());

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
}
