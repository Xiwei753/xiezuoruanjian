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
#[allow(clippy::too_many_lines)]
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
    // #644 评论 5475413230 第3节 + #644 评论 5483920624 问题4：
    // 从 HEAD 读真实目标，不猜 main。unborn 的 HEAD 仍然是 symbolic ref，
    // 可能指向 main、master 或其它分支。
    //
    // 注意：git2 的 HEAD 在首次 commit 后可能变成 direct reference
    // （symbolic_target() == None），但仍然是分支 HEAD（is_branch() == true），
    // 不是 detached HEAD。只有 is_branch() == false 且有 target OID 时才是真正的
    // detached HEAD。
    //
    // #644 评论 5483920624 问题4：不吞错——
    // - shorthand 取不到 → Err（HEAD 读取失败是真实错误）。
    // - HEAD 存在但既无 symbolic 也无 direct target → Err（不伪造 unborn main）。
    // - head() 返回 Err：只有 libgit2 明确的 UnbornBranch 语义才走 Unborn，
    //   且 head_ref 必须从 .git/HEAD 文件真实解析出来；其它错误返回 Err。
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
                // #644 评论 5483920624 问题4：shorthand 取不到 → Err，不 fallback main。
                let shorthand = head.shorthand().ok_or_else(|| {
                    crate::Error::Io(std::io::Error::other(
                        "seed_from_live_as_git_repo: HEAD is branch but shorthand() \
                         returned None — cannot determine branch name, refusing to \
                         guess",
                    ))
                })?;
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
                // #644 评论 5483920624 问题4：HEAD 存在但既无 symbolic 也无
                // direct target — 不伪造 unborn main，返回 Err。
                return Err(crate::Error::Io(std::io::Error::other(
                    "seed_from_live_as_git_repo: HEAD exists but has neither symbolic \
                     target nor direct target — refusing to guess unborn main",
                )));
            }
        }
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => {
            // #644 评论 5483920624 问题4：只有 libgit2 明确的 UnbornBranch 语义
            // 才走 Unborn，且 head_ref 必须从 .git/HEAD 文件真实解析出来。
            let head_file = live_root.join(".git").join("HEAD");
            let content = fs::read_to_string(&head_file).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "seed_from_live_as_git_repo: UnbornBranch but failed to read \
                     .git/HEAD: {e}"
                )))
            })?;
            let trimmed = content.trim();
            let head_ref = trimmed
                .strip_prefix("ref: ")
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "seed_from_live_as_git_repo: UnbornBranch but .git/HEAD content \
                     is not 'ref: ...' format (got {:?}) — refusing to guess unborn main",
                        trimmed
                    )))
                })?;
            GitSeedState::Unborn { head_ref }
        }
        Err(e) => {
            // #644 评论 5483920624 问题4：其它 head() 错误（NotFound、IO 等）
            // 不伪造 unborn main，返回 Err。
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "seed_from_live_as_git_repo: failed to read HEAD: {e}"
            ))));
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

// #644 评论 5475805198 第2节：finalize 逻辑已移至 git_commit.rs。
// 本模块只保留 seed / GitSeedState。

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

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
        )
        .unwrap();
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

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
        )
        .unwrap();
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

        let run = StagingRun::create(
            tmp.path(),
            live.clone(),
            crate::sync::types::BackendType::Git,
        )
        .unwrap();
        let seed_state = seed_from_live_as_git_repo(&run, &live).unwrap();
        // unborn → 返回 Unborn
        assert!(matches!(seed_state, GitSeedState::Unborn { .. }));

        // staging 拿到了 .git/ 元数据
        assert!(run.staging_root().join(".git").exists());
    }
}
