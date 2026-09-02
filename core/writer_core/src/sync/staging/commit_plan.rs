use std::path::PathBuf;

/// Commit plan — Commit 阶段对每个 staging 变化的处理决策。
///
/// #644 评论 5474166587 问题1：拆 `content_actions` + `engine_state_actions`。
/// - `content_actions`：用户内容（正文、元数据、缓存）的写回动作。
/// - `engine_state_actions`：同步引擎自身状态（manifest.sync.json、
///   state.local.json、conflicts.json）的写回动作。
///
/// 两类最后用同一个 `SaveTransaction` 一次写回 live，不另起第二套保存路径。
#[derive(Default, Debug)]
pub struct CommitPlan {
    /// 用户内容写回动作：local==base 时安全应用 incoming（含 incoming 独有新增）。
    pub content_actions: Vec<CommitAction>,
    /// 引擎状态写回动作：app-meta/sync/manifest.sync.json、state.local.json、
    /// conflicts.json 等。Transfer 在 staging 里更新了它们，Commit 必须写回 live。
    pub engine_state_actions: Vec<CommitAction>,
    /// incoming==base，保留 local（无需动作，记录供诊断）。
    pub keep_local: Vec<PathBuf>,
    /// local==incoming，内容相同，无需操作。
    pub noop: Vec<PathBuf>,
    /// 两边都改，三方冲突（正文走三方合并语义，metadata 走 LWW，由调用方决定）。
    /// #644 评论 5473401065 第4节：用 `StagingConflict` 替代 `PathBuf`，
    /// 保留 base/local/incoming 哈希，让 Commit 阶段能映射成 `SyncConflict` 并持久化。
    pub conflict: Vec<StagingConflict>,
}

/// #644 评论 5473401065 第4节：三方冲突的完整信息。
///
/// 保留 `rel_path` + 三方哈希，Commit 阶段映射成 `SyncConflict` 时不再丢失信息。
#[derive(Debug, Clone)]
pub struct StagingConflict {
    pub rel_path: PathBuf,
    pub base_hash: String,
    pub local_hash: String,
    pub incoming_hash: String,
}

/// 单个文件的 commit 动作。
#[derive(Debug, Clone)]
pub enum CommitAction {
    /// 把 `content` 写到 `rel_path`（相对 target_root）。
    Apply { rel_path: PathBuf, content: Vec<u8> },
    /// 删除 `rel_path`（远端删除，local 没改）。
    Delete { rel_path: PathBuf },
}

/// #644 评论 5474166587 问题1：staging commit 写回语义分类。
///
/// 与 [`ContentClass`]（远端同步语义）正交。决定 Transfer 在 staging 里产生的
/// 哪些本地状态必须写回 live：
/// - `Content`：用户内容，走三方比较/LWW 决策。
/// - `EngineState`：同步引擎自身状态（manifest/state/conflicts），直接写回 live。
/// - `Skip`：永不进 commit（.git/、full-sync-staging/、app-meta/transactions/、
///   config.local.json、secrets）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingCommitClass {
    /// 用户内容：正文、元数据、缓存。走三方比较/LWW 决策。
    Content,
    /// 引擎状态：manifest.sync.json、state.local.json、conflicts.json。
    /// Transfer 在 staging 里更新了它们，Commit 必须写回 live。
    EngineState,
    /// 永不进 commit：.git/、full-sync-staging/、app-meta/transactions/、
    /// config.local.json、secrets。
    Skip,
}

/// 判断路径是否为内部 Git 工件（不应被当成用户内容同步）。
///
/// 统一过滤以下模式：
/// - `.git`（精确匹配）：Git 仓库元数据目录或 gitlink 文件；
/// - `.git/`（前缀匹配）：Git 仓库元数据子目录；
/// - `.git.sujian-tmp-*`：迁移/恢复过程中的临时目录；
/// - `.git.sujian-migrate-source-*`：迁移崩溃后残留的源仓库快照。
pub(crate) fn is_internal_git_artifact(path: &str) -> bool {
    // Normalize backslashes to forward slashes for consistent matching on Windows
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

/// #644 评论 5474166587 问题1：按 staging commit 写回语义分类。
///
/// 与 [`crate::sync::content_class::classify_content_path`]（远端同步语义）正交。
/// `app-meta/` 下只有 `sync/manifest.sync.json`、`sync/state.local.json`、
/// `sync/conflicts.json` 是 EngineState，其余 app-meta 内容（如 transactions/、
/// logs/）不进 commit。
pub(crate) fn classify_staging_commit_path(raw_path: &str) -> StagingCommitClass {
    // Normalize backslashes to forward slashes for consistent matching on Windows
    let path = if raw_path.contains('\\') {
        std::borrow::Cow::Owned(raw_path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(raw_path)
    };

    // 永不进 commit 的内部目录（walk_commit_candidates 已跳过，这里兜底）。
    // 使用统一过滤函数判断 Git 工件。
    if is_internal_git_artifact(&path) {
        return StagingCommitClass::Skip;
    }
    if path.starts_with("full-sync-staging/") || path == "full-sync-staging" {
        return StagingCommitClass::Skip;
    }
    if path.starts_with("app-meta/transactions/") {
        return StagingCommitClass::Skip;
    }

    // EngineState：同步引擎自身状态，Transfer 在 staging 里更新了它们，Commit 必须写回 live。
    if path == "app-meta/sync/manifest.sync.json"
        || path == "app-meta/sync/state.local.json"
        || path == "app-meta/sync/conflicts.json"
    {
        return StagingCommitClass::EngineState;
    }

    // 平台配置/凭证：不从 staging 覆盖 live（设备专属）。
    if path == "app-meta/sync/config.local.json" || path.starts_with("app-meta/sync/secrets") {
        return StagingCommitClass::Skip;
    }

    // 其余 app-meta/ 内容（logs/、stats/ 等）不进 commit。
    if path.starts_with("app-meta/") {
        return StagingCommitClass::Skip;
    }

    StagingCommitClass::Content
}

/// 比较两个 `Option<Vec<u8>>` 是否相等。
/// `None == None` → true，`None == Some(_)` → false。
pub(crate) fn opt_bytes_eq(a: &Option<Vec<u8>>, b: &Option<Vec<u8>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    }
}

/// #644 评论 5473789298 第3节：把 incoming 内容推入 plan 的 actions 列表。
///
/// `incoming = Some` → [`CommitAction::Apply`]；`incoming = None`（远端删除）→
/// [`CommitAction::Delete`]。按 `class` 决定推入 `content_actions` 还是
/// `engine_state_actions`。
pub(crate) fn apply_incoming(
    plan: &mut CommitPlan,
    rel: PathBuf,
    incoming: Option<Vec<u8>>,
    class: StagingCommitClass,
) {
    let action = match incoming {
        Some(content) => CommitAction::Apply {
            rel_path: rel,
            content,
        },
        None => CommitAction::Delete { rel_path: rel },
    };
    match class {
        StagingCommitClass::EngineState => plan.engine_state_actions.push(action),
        StagingCommitClass::Content => plan.content_actions.push(action),
        StagingCommitClass::Skip => {
            // classify_staging_commit_path 已过滤 Skip，不应到达此处。
            // 防御性丢弃，不写回 live。
        }
    }
}
