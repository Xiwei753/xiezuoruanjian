//! #645 评论 5504296097 问题1/4：workspace 路径分类的统一事实来源。
//!
//! 本模块属于 `storage` 层，不依赖 `sync`。`sync::staging` 与
//! `storage::workspace_git` 都委托到这里，消除 `storage/workspace_git ->
//! sync/staging` 的反向依赖。
//!
//! #645 评论 5504296097 问题4：把路径规则从一个 bool 拆成真正的分类
//! [`WorkspacePathClass`]，再分别决定 [`is_sync_staging_path`] 与
//! [`is_workspace_history_path`]：
//! - `UserContent` / `UserSetting` → 可以进本地 Git history，也可以进 staging；
//! - `SyncEngineState` → staging 可以使用，不进本地 Git history；
//! - `Secret` / `Cache` / `InternalRuntime` → staging/history 都不进。
//!
//! 规则统一来源：
//! - `sync/staging/commit_plan.rs::is_internal_git_artifact`：
//!   `.git`、`.git/`、`.git.sujian-tmp-*`、`.git.sujian-migrate-source-*`；
//! - `storage/workspace_git/history.rs::stage_dir_recursive` 排除规则：
//!   `full-sync-staging`、`app-meta/transactions`、`secrets`、`app-meta/logs`、
//!   `cache`、`.tmp`、`.lock`；
//! - `sync/staging/run.rs::walk_commit_candidates` 跳过规则：
//!   `.git*`、`full-sync-staging`、`app-meta/transactions`；
//! - #645 评论 5504296097 问题3：`sync/trash`（删除回收站）和
//!   `app-meta/delete-journals`（删除事务 journal）也归 `InternalRuntime`，
//!   staging/history 都不进。

use std::path::Path;

/// #645 评论 5504296097 问题4：workspace 路径分类。
///
/// 把路径规则从一个 bool 拆成真正的分类，再分别决定
/// [`is_sync_staging_path`] 与 [`is_workspace_history_path`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePathClass {
    /// 用户内容（projects/...、starmaps/... 等）。可进 staging 与本地 Git history。
    UserContent,
    /// 用户设置（settings.local.json、settings.sync.json 等）。可进 staging 与本地 Git history。
    UserSetting,
    /// 同步引擎运行状态（app-meta/sync/manifest.sync.json、state.local.json、
    /// config.local.json、conflicts.json、full_state.local.json、
    /// pending_deleted_targets.json）。staging 可以使用，不进本地 Git history。
    SyncEngineState,
    /// 凭据（secrets*.local.json）。staging/history 都不进。
    Secret,
    /// 缓存（cache/、.tmp、.lock）。staging/history 都不进。
    Cache,
    /// 内部运行时（.git、full-sync-staging、app-meta/transactions、app-meta/logs、
    /// sync/trash、app-meta/delete-journals）。staging/history 都不进。
    InternalRuntime,
}

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

/// 将路径分隔符统一为 `/`，便于跨平台比较。
fn normalize_workspace_path(path: &str) -> String {
    if path.contains('\\') {
        path.replace('\\', "/")
    } else {
        path.to_string()
    }
}

/// 判断路径是否为 workspace 内部凭据文件。
///
/// #645 评论 5504296097 Blocker 1：真实凭据文件名是
/// `secrets.local.json` / `secrets_g1.local.json` / `secrets_g2.local.json`，
/// 路径段是 `secrets.local.json` 而非 `secrets`，原段名匹配 `seg == "secrets"`
/// 会漏判，导致凭据被写进本地 Git 历史。
///
/// 规则只针对真正的内部凭据路径，不用 `contains("secret")` 误伤
/// 用户自己的 `my-secrets-book` 目录。同时覆盖旧版凭据文件名
/// `sync_secrets.local.json`（迁移残留）。
fn is_workspace_secret_path(path: &str) -> bool {
    let path = normalize_workspace_path(path);
    path == "app-meta/sync/secrets.local.json"
        || path.starts_with("app-meta/sync/secrets_g")
        || path.starts_with("app-meta/sync/secrets.")
        || path.ends_with("sync_secrets.local.json")
}

/// #645 评论 5504296097 问题4：判断路径是否为同步引擎运行状态文件。
///
/// 这些文件是远端同步自己的运行状态，不是用户 workspace 本地版本历史内容：
/// - `app-meta/sync/manifest.sync.json`：同步清单；
/// - `app-meta/sync/state.local.json`：App target 同步状态；
/// - `app-meta/sync/full_state.local.json`：全量同步持久状态；
/// - `app-meta/sync/conflicts.json`：冲突记录；
/// - `app-meta/sync/config.local.json`：同步配置；
/// - `app-meta/sync/pending_deleted_targets.json`：待删除 target tombstone 列表
///   （#645 评论 5504296097 问题4：deleted target durable handoff 的 provider-neutral
///   持久状态，由 sync engine 自己读写，不是用户内容）。
///
/// staging 可以使用这些文件（同步需要），但 [`is_workspace_history_path`]
/// 必须排除它们，避免把远端同步运行状态当成用户内容写进本地 Git history。
fn is_sync_engine_state_path(path: &str) -> bool {
    let path = normalize_workspace_path(path);
    path == "app-meta/sync/manifest.sync.json"
        || path == "app-meta/sync/state.local.json"
        || path == "app-meta/sync/full_state.local.json"
        || path == "app-meta/sync/conflicts.json"
        || path == "app-meta/sync/config.local.json"
        || path == "app-meta/sync/pending_deleted_targets.json"
}

/// [`is_sync_engine_state_path`] 的 `&Path` 版本。
fn is_sync_engine_state_path_p(path: &Path) -> bool {
    is_sync_engine_state_path(&path.to_string_lossy())
}

/// 判断路径是否为 workspace 内部路径（不应被当成用户内容）。
///
/// 统一过滤以下模式：
/// - Git 工件（委托 [`is_internal_git_artifact`]）；
/// - `full-sync-staging`（含子目录）：staging run 自身，避免递归；
/// - `app-meta/transactions`（含子目录）：事务暂存目录，commit 中间态；
/// - `app-meta/logs`（含子目录）：本地日志；
/// - `sync/trash`（含子目录）：删除回收站，运行时数据；
/// - `app-meta/delete-journals`（含子目录）：删除事务 journal，崩溃恢复数据；
/// - 真实内部凭据路径（委托 [`is_workspace_secret_path`]）：永不进历史；
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

    // #645 评论 5504296097 问题3：sync/trash/（删除回收站，运行时数据）
    if normalized == "sync/trash" || normalized.starts_with("sync/trash/") {
        return true;
    }

    // #645 评论 5504296097 问题3：app-meta/delete-journals/（删除事务 journal，崩溃恢复数据）
    if normalized == "app-meta/delete-journals"
        || normalized.starts_with("app-meta/delete-journals/")
    {
        return true;
    }

    // secrets：真实内部凭据路径（app-meta/sync/secrets*.local.json 等）。
    // #645 评论 5504296097 Blocker 1：原段名匹配 seg == "secrets" 漏判
    // secrets.local.json，改成精确路径匹配。
    if is_workspace_secret_path(&normalized) {
        return true;
    }

    // cache：路径中包含该段时视为内部。
    // 用 split('/') 段级匹配，避免误判名为 "my-cache-book" 的用户目录。
    for seg in normalized.split('/') {
        if seg == "cache" {
            return true;
        }
    }

    // .tmp / .lock 后缀
    if normalized.ends_with(".tmp") || normalized.ends_with(".lock") {
        return true;
    }

    false
}

/// 判断路径是否允许进入本地版本历史（非内部/非 secrets/非 cache/非 log/非 runtime/非 sync engine state）。
///
/// #645 评论 5504296097 问题4：不再等于 `!is_workspace_internal_path`，
/// 而是进一步排除 [`is_sync_engine_state_path`]。sync engine state
/// （manifest.sync.json / state.local.json / config.local.json /
/// conflicts.json / full_state.local.json）staging 可以用，但不进本地 Git history。
/// 所有进入 `record_workspace_changes` 的路径（无论来自显式参数还是全量扫描）
/// 都必须先走本函数；被排除的路径直接跳过。
pub fn is_workspace_history_path(path: &Path) -> bool {
    !is_workspace_internal_path(path) && !is_sync_engine_state_path_p(path)
}

/// [`is_workspace_history_path`] 的 `&str` 版本。
pub fn is_workspace_history_path_str(path: &str) -> bool {
    !is_workspace_internal_path_str(path) && !is_sync_engine_state_path(path)
}

/// #645 评论 5504296097 问题4：判断路径是否允许进入同步 staging。
///
/// staging 可以看见用户内容、用户设置和同步引擎状态，但不能看见
/// 凭据、缓存和内部运行时。等于 `!is_workspace_internal_path`，
/// 保留向后兼容语义。
pub fn is_sync_staging_path(path: &Path) -> bool {
    !is_workspace_internal_path(path)
}

/// [`is_sync_staging_path`] 的 `&str` 版本。
pub fn is_sync_staging_path_str(path: &str) -> bool {
    !is_workspace_internal_path_str(path)
}

/// #645 评论 5504296097 问题4：把路径分类成 [`WorkspacePathClass`]。
///
/// 分类顺序：先判 InternalRuntime（Git 工件/full-sync-staging/transactions/logs/
/// sync/trash/app-meta/delete-journals/.tmp/.lock），再判 Secret（凭据），
/// 再判 Cache，再判 SyncEngineState（sync engine state 文件），
/// 剩余按是否在已知用户设置路径下判 UserSetting，其余为 UserContent。
pub fn classify_workspace_path(path: &Path) -> WorkspacePathClass {
    let rel_str = path.to_string_lossy();
    classify_workspace_path_str(&rel_str)
}

/// [`classify_workspace_path`] 的 `&str` 版本。
pub fn classify_workspace_path_str(path: &str) -> WorkspacePathClass {
    let normalized = normalize_workspace_path(path);

    // InternalRuntime：Git 工件、full-sync-staging、transactions、logs、
    // #645 评论 5504296097 问题3：sync/trash、app-meta/delete-journals、.tmp、.lock
    if is_internal_git_artifact(&normalized)
        || normalized == "full-sync-staging"
        || normalized.starts_with("full-sync-staging/")
        || normalized == "app-meta/transactions"
        || normalized.starts_with("app-meta/transactions/")
        || normalized == "app-meta/logs"
        || normalized.starts_with("app-meta/logs/")
        || normalized == "sync/trash"
        || normalized.starts_with("sync/trash/")
        || normalized == "app-meta/delete-journals"
        || normalized.starts_with("app-meta/delete-journals/")
        || normalized.ends_with(".tmp")
        || normalized.ends_with(".lock")
    {
        return WorkspacePathClass::InternalRuntime;
    }

    // Secret：凭据
    if is_workspace_secret_path(&normalized) {
        return WorkspacePathClass::Secret;
    }

    // Cache：路径中包含 cache 段
    for seg in normalized.split('/') {
        if seg == "cache" {
            return WorkspacePathClass::Cache;
        }
    }

    // SyncEngineState：同步引擎运行状态
    if is_sync_engine_state_path(&normalized) {
        return WorkspacePathClass::SyncEngineState;
    }

    // UserSetting：已知用户设置文件
    if normalized == "settings.local.json"
        || normalized == "settings.sync.json"
        || normalized.starts_with("settings/")
    {
        return WorkspacePathClass::UserSetting;
    }

    // 其余为 UserContent
    WorkspacePathClass::UserContent
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
            "app-meta/sync/secrets.local.json"
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
        // #645 评论 5504296097 问题4：sync engine state 不再是 history path。
        assert!(!is_workspace_history_path(&PathBuf::from(
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

    /// #645 评论 5504296097 Blocker 1：真实凭据文件必须被识别为内部路径。
    #[test]
    fn secret_paths_are_internal() {
        assert!(is_workspace_internal_path_str(
            "app-meta/sync/secrets.local.json"
        ));
        assert!(is_workspace_internal_path_str(
            "app-meta/sync/secrets_g1.local.json"
        ));
        assert!(is_workspace_internal_path_str(
            "app-meta/sync/secrets_g2.local.json"
        ));
        assert!(is_workspace_internal_path_str(
            "app-meta/sync/sync_secrets.local.json"
        ));
        // 旧版凭据可能出现在 settings 目录下。
        assert!(is_workspace_internal_path_str(
            "settings/sync_secrets.local.json"
        ));
        // Windows 风格路径也要识别。
        assert!(is_workspace_internal_path_str(
            "app-meta\\sync\\secrets.local.json"
        ));
        // 用户命名的 my-secrets-book 不应被误判。
        assert!(!is_workspace_internal_path_str(
            "projects/my-secrets-book/ch1.md"
        ));
        // #645 评论 5504296097 问题4：sync manifest 不再允许进历史
        // （归为 SyncEngineState，staging 可用但 history 排除）。
        assert!(!is_workspace_history_path_str(
            "app-meta/sync/manifest.sync.json"
        ));
        // 但 staging 仍可用。
        assert!(is_sync_staging_path_str("app-meta/sync/manifest.sync.json"));
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

    /// #645 评论 5504296097 问题4：WorkspacePathClass 分类。
    #[test]
    fn classify_paths() {
        use WorkspacePathClass::*;
        // UserContent
        assert_eq!(
            classify_workspace_path(&PathBuf::from("projects/p1/chapter.md")),
            UserContent
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("starmaps/sm_1.meta.json")),
            UserContent
        );
        // UserSetting
        assert_eq!(
            classify_workspace_path(&PathBuf::from("settings.local.json")),
            UserSetting
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("settings.sync.json")),
            UserSetting
        );
        // SyncEngineState
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/sync/manifest.sync.json")),
            SyncEngineState
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/sync/state.local.json")),
            SyncEngineState
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/sync/config.local.json")),
            SyncEngineState
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/sync/full_state.local.json")),
            SyncEngineState
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/sync/conflicts.json")),
            SyncEngineState
        );
        // #645 评论 5504296097 问题4：pending_deleted_targets.json 归
        // SyncEngineState（staging 可用、不进本地 Git history）。
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/sync/pending_deleted_targets.json")),
            SyncEngineState
        );
        assert!(!is_workspace_history_path_str(
            "app-meta/sync/pending_deleted_targets.json"
        ));
        assert!(is_sync_staging_path_str(
            "app-meta/sync/pending_deleted_targets.json"
        ));
        // Secret
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/sync/secrets.local.json")),
            Secret
        );
        // Cache
        assert_eq!(
            classify_workspace_path(&PathBuf::from("cache/index.bin")),
            Cache
        );
        // InternalRuntime
        assert_eq!(
            classify_workspace_path(&PathBuf::from(".git/HEAD")),
            InternalRuntime
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("full-sync-staging")),
            InternalRuntime
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/transactions/tx1")),
            InternalRuntime
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("scratch.tmp")),
            InternalRuntime
        );
        // #645 评论 5504296097 问题3：sync/trash 和 app-meta/delete-journals
        // 归 InternalRuntime，staging/history 都不进。
        assert_eq!(
            classify_workspace_path(&PathBuf::from("sync/trash")),
            InternalRuntime
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("sync/trash/deleted-volume")),
            InternalRuntime
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from("app-meta/delete-journals")),
            InternalRuntime
        );
        assert_eq!(
            classify_workspace_path(&PathBuf::from(
                "app-meta/delete-journals/.sujian-delete-journal-xxx"
            )),
            InternalRuntime
        );
        assert!(is_workspace_internal_path(&PathBuf::from(
            "sync/trash/anything"
        )));
        assert!(is_workspace_internal_path(&PathBuf::from(
            "app-meta/delete-journals/anything"
        )));
        assert!(!is_workspace_history_path(&PathBuf::from(
            "sync/trash/anything"
        )));
        assert!(!is_workspace_history_path(&PathBuf::from(
            "app-meta/delete-journals/anything"
        )));
    }

    /// #645 评论 5504296097 问题4：is_sync_staging_path 与 is_workspace_history_path
    /// 在 SyncEngineState 上分歧——staging 可用，history 排除。
    #[test]
    fn staging_vs_history_diverge_on_sync_engine_state() {
        let p = PathBuf::from("app-meta/sync/state.local.json");
        assert!(is_sync_staging_path(&p), "sync engine state 应可进 staging");
        assert!(
            !is_workspace_history_path(&p),
            "sync engine state 不应进本地 Git history"
        );
        // 用户内容两者都 true
        let user = PathBuf::from("projects/p1/chapter.md");
        assert!(is_sync_staging_path(&user));
        assert!(is_workspace_history_path(&user));
        // 凭据两者都 false
        let secret = PathBuf::from("app-meta/sync/secrets.local.json");
        assert!(!is_sync_staging_path(&secret));
        assert!(!is_workspace_history_path(&secret));
    }
}
