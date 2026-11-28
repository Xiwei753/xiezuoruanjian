use std::path::PathBuf;

/// Commit plan — Commit 阶段对每个 staging 变化的处理决策。
///
///   拆 `content_actions` `engine_state_actions`。
/// - `content_actions`：用户内容（正文、元数据、缓存）的写回动作。
/// - `engine_state_actions`：同步引擎自身状态（最终对象索引）的写回动作。
///   `state.local.json` / `conflicts.json` 永不从 staging 复制或三方合并；
///   提交阶段只写本轮真实裁决产生的记录（Issue #1066）。
///
/// 两类最后用同一个 `SaveTransaction` 一次写回 live，不另起第二套保存路径。
#[derive(Default, Debug)]
pub struct CommitPlan {
    /// 本 plan 由统一对象比较产出（Issue #1066）。
    ///
    /// 提交阶段据此走对象提交：只推进远端已确认对象的基线，冲突/上传等待
    /// 回执确认；不再走旧的 state 三方合并。旧生命周期路径仍产出 false 的 plan。
    pub object_engine: bool,
    /// 用户内容写回动作：本地未改时应用远端内容/删除（含远端独有新增）。
    pub content_actions: Vec<CommitAction>,
    /// 引擎状态写回动作：最终对象索引（manifest，canonical JSON）。
    /// 提交阶段与正文内容在同一个 `SaveTransaction` 写回 live。
    pub engine_state_actions: Vec<CommitAction>,
    /// 需要上传的本地对象（本地相对路径）。
    ///
    /// 由提交阶段冻结内容并生成不可变发布决议；传输/发布不再自己猜上传清单。
    pub uploads: Vec<PathBuf>,
    /// 需要把本地删除传播到远端的对象（本地相对路径）。
    pub publish_deletes: Vec<PathBuf>,
    /// 只有本地有变化、远端未参与：保留本地（无需写盘，上传由 `uploads` 表达）。
    pub keep_local: Vec<PathBuf>,
    /// 双侧内容相同，无需操作。
    pub noop: Vec<PathBuf>,
    /// 双侧都改变且结果不同：保留双方原始内容，交给用户选择。
    pub conflict: Vec<StagingConflict>,
}

/// 三方冲突的完整信息。
///
/// 保留 `rel_path` + 三方哈希，Commit 阶段映射成 `SyncConflict` 时不再丢失信息。
///
/// 字段语义：
/// - `rel_path`：incoming 内容在 staging_root 下的相对路径（也是 live target 下的相对路径）。
/// - `kind`：冲突语义（`BothChanged` 双端修改 / `RemoteDeleted` 远端删除+本地修改）。
///   `compute_commit_plan` 在 `ThreeWayResult::BothChanged` 分支按 `incoming.is_some()`
///   显式区分，不再靠 `remote_snapshot_path` 是否存在猜语义（#757）。
/// - `remote_snapshot_path`：在 commit_helpers 的 `run.cleanup()` 之前由
///   `save_conflict_copy` 填入，是相对 live_root 的快照路径（如
///   `volumes/v1/chapters/c1/chapter.md.remote-conflict-20260925-012345`）。
///   `record_staging_conflicts` 据此填 `SyncConflict.remote_snapshot_path`，
///   让 #757 冲突侧栏"用户看到什么就选择什么"——预览展示的远端正文与
///   `take_remote` 实际采用的正文是同一份。`None` 表示未保存快照（老数据兼容，
///   `take_remote` 回退 `pending_take_remote`）。`RemoteDeleted` 时恒为 `None`
///   （远端已删除，无 incoming 正文可保存）。
#[derive(Debug, Clone)]
pub struct StagingConflict {
    pub rel_path: PathBuf,
    pub base_hash: String,
    pub local_hash: String,
    pub incoming_hash: String,
    /// 冲突语义：BothChanged（双端修改）或 RemoteDeleted（远端删除+本地修改）。
    /// 不再靠 remote_snapshot_path 是否存在猜语义。
    pub kind: crate::sync::types::SyncConflictKind,
    pub remote_snapshot_path: Option<String>,
    /// graph 级 BothChanged 冲突时保存的远端整图快照目录（相对 sync_root）。
    ///
    /// **issue #902 第十四轮复核**：当 `rel_path` 是 `starmaps/<id>/graph.json`
    /// 且 `kind == BothChanged`（本地删除远端修改，用户选远端 = 恢复远端整图），
    /// `save_conflict_snapshots` 会把 staging_root 下该星图的**所有子对象文件**
    /// 复制到 `<sync_root>/.conflict-snapshot/starmaps/<id>/` 下，本字段记录该
    /// 目录相对 sync_root 的路径。`apply_starmap_graph_remote_modify` 据此恢复
    /// 远端整图。非 graph 级冲突或 `RemoteDeleted` 时为 `None`。
    pub remote_graph_snapshot_dir: Option<String>,
}

/// 单个文件的 commit 动作。
#[derive(Debug, Clone)]
pub enum CommitAction {
    /// 把 `content` 写到 `rel_path`（相对 target_root）。
    Apply { rel_path: PathBuf, content: Vec<u8> },
    /// 删除 `rel_path`（远端删除，local 没改）。
    Delete { rel_path: PathBuf },
}

///   staging commit 写回语义分类。
///
/// 与 [`ContentClass`]（远端同步语义）正交。决定 Transfer 在 staging 里产生的
/// 哪些本地状态必须写回 live：
/// - `Content`：用户内容，走三方比较/LWW 决策。
/// - `EngineState`：同步引擎自身状态（`manifest.sync.json`），直接写回 live。
/// - `Skip`：永不进 commit（.git/、full-sync-staging/、app-meta/transactions/、
///   config.local.json、secrets）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingCommitClass {
    /// 用户内容：正文、元数据、缓存。走三方比较/LWW 决策。
    Content,
    /// 引擎状态：`manifest.sync.json`。Transfer 在 staging 里更新了它，
    /// Commit 必须写回 live。
    EngineState,
    /// 永不进 commit：.git/、full-sync-staging/、app-meta/transactions/、
    /// config.local.json、secrets。
    Skip,
}

/// 判断路径是否为内部 Git 工件（不应被当成用户内容同步）。
///
///   规则统一到
/// [`crate::storage::workspace_paths::is_internal_git_artifact`]，
/// 本函数保留为薄包装（sync 内部仍在用），不再持有规则副本，
/// 消除 `storage/workspace_git -> sync/staging` 反向依赖。
pub(crate) fn is_internal_git_artifact(path: &str) -> bool {
    crate::storage::workspace_paths::is_internal_git_artifact(path)
}

///   按 staging commit 写回语义分类。
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
    // manifest.sync.json 继续走 EngineState 直接写回（同步引擎管理的远端 LWW 元数据）。
    if path == "app-meta/sync/manifest.sync.json" {
        return StagingCommitClass::EngineState;
    }
    // state.local.json / conflicts.json 只由提交阶段按本轮真实裁决写入，
    // 永不从 staging 复制或做三方合并（Issue #1066）。
    if path == "app-meta/sync/state.local.json" || path == "app-meta/sync/conflicts.json" {
        return StagingCommitClass::Skip;
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

/// App group variant of the staging classifier. Group-local manifest/state/conflicts
/// are routed through this run, while the old aggregate App metadata remains a
/// read-only migration input and is never replayed by a group commit.
pub(crate) fn classify_staging_commit_path_for_group(
    raw_path: &str,
    group: &crate::sync::types::AppDataGroup,
) -> StagingCommitClass {
    let path = raw_path.replace('\\', "/");
    if path == group.local_manifest_path() {
        return StagingCommitClass::EngineState;
    }
    if path == group.local_state_path() || path == group.local_conflicts_path() {
        return StagingCommitClass::Skip;
    }

    // Legacy aggregate files can be read by group migration code but are not owned
    // by a single group and must not be written back from this staging run.
    if path == "app-meta/sync/manifest.sync.json"
        || path == "app-meta/sync/state.local.json"
        || path == "app-meta/sync/conflicts.json"
    {
        return StagingCommitClass::Skip;
    }

    if group.owns_path(&path) {
        classify_staging_commit_path(&path)
    } else {
        StagingCommitClass::Skip
    }
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

/// 把 incoming 内容推入 plan 的 actions 列表。
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
