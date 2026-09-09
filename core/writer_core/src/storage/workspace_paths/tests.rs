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
    //   sync engine state 不再是 history path。
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

/// 真实凭据文件必须被识别为内部路径。
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
    //   sync manifest 不再允许进历史
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

///   WorkspacePathClass 分类。
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
    //   pending_deleted_targets.json 归
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
    //   sync/trash 和 app-meta/delete-journals
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

///   is_sync_staging_path 与 is_workspace_history_path
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
