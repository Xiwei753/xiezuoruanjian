//! #645 评论 5504296097 问题1：workspace 路径分类的统一事实来源。
//!
//! 本模块属于 `storage` 层，不依赖 `sync`。`sync::staging` 与
//! `storage::workspace_git` 都委托到这里，消除 `storage/workspace_git ->
//! sync/staging` 的反向依赖。
//!
//! 提供两个正交的判断函数：
//! - [`is_workspace_internal_path`]：路径是否为 workspace 内部路径
//!   （不应被当成用户内容同步/进入本地版本历史）。
//! - [`is_workspace_history_path`]：路径是否允许进入本地版本历史
//!   （非内部/非 secrets/非 cache/非 log/非 runtime）。
//!
//! 规则统一来源：
//! - `sync/staging/commit_plan.rs::is_internal_git_artifact`：
//!   `.git`、`.git/`、`.git.sujian-tmp-*`、`.git.sujian-migrate-source-*`；
//! - `storage/workspace_git/history.rs::stage_dir_recursive` 排除规则：
//!   `full-sync-staging`、`app-meta/transactions`、`secrets`、`app-meta/logs`、
//!   `cache`、`.tmp`、`.lock`；
//! - `sync/staging/run.rs::walk_commit_candidates` 跳过规则：
//!   `.git*`、`full-sync-staging`、`app-meta/transactions`。

use std::path::Path;

/// 判断路径是否为 Git 工件（`.git`、`.git.sujian-tmp-*`、`.git.sujian-migrate-source-*`）。
///
/// 这是 [`is_workspace_internal_path`] 的子规则，单独暴露供 `sync::staging`
/// 复用，避免重复实现导致规则漂移。
///
/// - `.git`（精确匹配）：Git 仓库元数据目录或 gitlink 文件；
/// - `.git/`（前缀匹配）：Git 仓库元数据子目录；
/// - `.git.sujian-tmp-*`：迁移/恢复过程中的临时目录；
/// - `.git.sujian-migrate-source-*`：迁移崩溃后残留的源仓库快照。
pub fn is_internal_git_artifact(path: &str) -> bool {
    // Normalize backslashes to forward slashes for consistent matching on Windows.
    let normalized = if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path.to_string()
    };

    // .git (exact) or .git/* (subdirectory)
    if normalized == ".git" || normalized.starts_with(".git/") {
        return true;
    }

    // .git.sujian-tmp-* (migration temp directory)
    if normalized.starts_with(".git.sujian-tmp-") {
        return true;
    }

    // .git.sujian-migrate-source-* (migration crash residual)
    if normalized.starts_with(".git.sujian-migrate-source-") {
        return true;
    }

    false
}

/// 判断路径是否为 workspace 内部路径（不应被当成用户内容）。
///
/// 统一过滤以下模式：
/// - Git 工件（委托 [`is_internal_git_artifact`]）；
/// - `full-sync-staging`（含子目录）：staging run 自身，避免递归；
/// - `app-meta/transactions`（含子目录）：事务暂存目录，commit 中间态；
/// - `app-meta/logs`（含子目录）：本地日志；
/// - 路径中包含 `secrets`：凭证/密钥，永不进历史；
/// - 路径中包含 `cache`：缓存目录；
/// - `.tmp` 后缀：临时文件；
/// - `.lock` 后缀：锁文件。
///
/// 输入既支持 `&str`（workspace-relative，使用 `/` 分隔）也支持 `&Path`
/// （会按平台分隔符规范化）。所有比较都在 `/`-分隔的 normalized 形式上做。
pub fn is_workspace_internal_path(path: &Path) -> bool {
    let rel_str = path.to_string_lossy();
    is_workspace_internal_path_str(&rel_str)
}

/// [`is_workspace_internal_path`] 的 `&str` 版本，避免重复 `to_string_lossy`。
pub fn is_workspace_internal_path_str(path: &str) -> bool {
    // Normalize backslashes to forward slashes for consistent matching on Windows.
    let normalized = if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path.to_string()
    };

    if is_internal_git_artifact(&normalized) {
        return true;
    }

    // full-sync-staging/（staging run 自身）
    if normalized == "full-sync-staging" || normalized.starts_with("full-sync-staging/") {
        return true;
    }

    // app-meta/transactions/（事务暂存目录）
    if normalized == "app-meta/transactions" || normalized.starts_with("app-meta/transactions/") {
        return true;
    }

    // app-meta/logs/（本地日志）
    if normalized == "app-meta/logs" || normalized.starts_with("app-meta/logs/") {
        return true;
    }

    // secrets / cache：路径中包含这些段时视为内部。
    // 用 split('/') 段级匹配，避免误判名为 "my-secrets-book" 的用户目录。
    for seg in normalized.split('/') {
        if seg == "secrets" || seg == "cache" {
            return true;
        }
    }

    // .tmp / .lock 后缀
    if normalized.ends_with(".tmp") || normalized.ends_with(".lock") {
        return true;
    }

    false
}

/// 判断路径是否允许进入本地版本历史（非内部/非 secrets/非 cache/非 log/非 runtime）。
///
/// 即 `!is_workspace_internal_path(path)`。所有进入 `record_workspace_changes`
/// 的路径（无论来自显式参数还是全量扫描）都必须先走本函数；被排除的路径直接跳过。
pub fn is_workspace_history_path(path: &Path) -> bool {
    !is_workspace_internal_path(path)
}

/// [`is_workspace_history_path`] 的 `&str` 版本。
pub fn is_workspace_history_path_str(path: &str) -> bool {
    !is_workspace_internal_path_str(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn git_artifacts_are_internal() {
        assert!(is_internal_git_artifact(".git"));
        assert!(is_internal_git_artifact(".git/HEAD"));
        assert!(is_internal_git_artifact(".git.sujian-tmp-abc"));
        assert!(is_internal_git_artifact(".git.sujian-migrate-source-xyz"));
        assert!(!is_internal_git_artifact("projects/p1/chapter.md"));
    }

    #[test]
    fn internal_paths_are_detected() {
        assert!(is_workspace_internal_path(&PathBuf::from(".git")));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "full-sync-staging"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "full-sync-staging/run-1/file"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "app-meta/transactions"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "app-meta/transactions/tx-1"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from("app-meta/logs")));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "app-meta/logs/sync.log"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "app-meta/secrets/token.json"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "cache/index.bin"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from("scratch.tmp")));
        assert!(is_workspace_internal_path(&PathBuf::from("repo.lock")));
    }

    #[test]
    fn user_paths_are_history_paths() {
        assert!(is_workspace_history_path(&PathBuf::from(
            "projects/p1/chapter.md"
        )));
        assert!(is_workspace_history_path(&PathBuf::from(
            "app-meta/sync/manifest.sync.json"
        )));
        assert!(is_workspace_history_path(&PathBuf::from(
            "settings/syncable.json"
        )));
    }

    #[test]
    fn user_named_with_secrets_substring_is_not_internal() {
        // 段级匹配：my-secrets-book 不应被当成 secrets 内部路径。
        assert!(!is_workspace_internal_path(&PathBuf::from(
            "projects/my-secrets-book/ch1.md"
        )));
        assert!(!is_workspace_internal_path(&PathBuf::from(
            "projects/p1/cache-recovery.md"
        )));
    }

    #[test]
    fn str_versions_match_path_versions() {
        let p = PathBuf::from("full-sync-staging/x");
        assert_eq!(
            is_workspace_internal_path(&p),
            is_workspace_internal_path_str("full-sync-staging/x")
        );
        assert_eq!(
            is_workspace_history_path(&p),
            is_workspace_history_path_str("full-sync-staging/x")
        );
    }
}
