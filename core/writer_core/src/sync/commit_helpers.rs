//! 同步 target 的 durable commit 决议与幂等发布。
//!
//! 一个 target 一次提交只有一份持久 [`SyncCommitManifest`]（`Decided`）：
//! 章节正文（正文/元数据/版本/证明事实）、state、对象索引、删除与冲突材料
//! 的最终决定全部先写进这份 manifest，再按条目逐个发布。进程中断后恢复入口
//! 只按这份决议继续发布尚未完成的条目，不重新比较“哪一边更新”。
//!
//! 章节正文的发布仍经过章节 journal（`commit_chapter_with_guard`）：journal
//! 负责 generation/revision/digest CAS 与正文投影幂等；sync manifest 负责
//! “本 target 这一轮到底决定了哪些章节/文件/删除”，并记录章节决议的
//! before/after 版本。因此不再出现“先逐章提交 journal，再补一个 state
//! SaveTransaction”的双发布顺序，也不会在 state 失败时把已写盘章节藏进
//! `Failed + committed_paths=[]`。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::chapter::{
    Chapter, ChapterChangeReceipt, ChapterCommitOutcome, ChapterCommitRequest, ChapterKey,
    ChapterMutationKind, ChapterSyncCommitReceipt, ChapterSyncProofFact, ChapterVersion,
};
use crate::sync::staging::{CommitAction, CommitPlan, PublishBatch, StagingRun};
use crate::sync::types::{
    AppDataGroup, SyncConflict, SyncConflictKind, SyncState, TargetSyncResult,
};

const SYNC_COMMIT_SCHEMA_VERSION: u32 = 1;
/// 固定路径：同一个 target 只存在一份持久决议。
const SYNC_COMMIT_DECISION_DIR: &str = "app-meta/sync/commit-decision";
const SYNC_COMMIT_MANIFEST_FILE: &str = "manifest.json";
const SYNC_COMMIT_MATERIALS_DIR: &str = "materials";

/// 把 target-relative `rel_path` 转成 workspace-relative path，供
/// `record_workspace_history` 精确 stage。
///
/// - App target：`live_root = app_data_root`，`rel_path` 已是 workspace-relative。
/// - Project target：`live_root = projects_root/<project_id>`，
///   workspace-relative = `projects/<project_id>/<rel_path>`。
/// - DeletedProject target with `RemoteTargetWins`：`target_kind` 保持为
///   `"deleted_project"`，但 `target_live_root` 已设为 `projects_root/<project_id>`。
fn to_workspace_rel_path(target_kind: &str, project_id: Option<&str>, rel_path: &Path) -> PathBuf {
    if target_kind == "project" || target_kind == "deleted_project" {
        if let Some(pid) = project_id {
            return PathBuf::from("projects").join(pid).join(rel_path);
        }
    }
    PathBuf::from(rel_path)
}

/// 同步 durable commit 实际写入 `correction/results/<chapter>/` 后，显式失效
/// 该章的校对结果索引；目录只读路径仍走 O(1) mtime 检查，不逐文件探测。
fn invalidate_correction_result_caches(
    app_data_root: &Path,
    live_root: &Path,
    project_id: Option<&str>,
    actions: &[crate::sync::staging::CommitAction],
) {
    let project_id = project_id
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            live_root
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        });
    let Some(project_id) = project_id else {
        return;
    };
    let mut chapters = std::collections::HashSet::new();
    for action in actions {
        let rel_path = match action {
            crate::sync::staging::CommitAction::Apply { rel_path, .. } => rel_path,
            crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
        };
        let normalized = rel_path.to_string_lossy().replace('\\', "/");
        let Some(rest) = normalized.strip_prefix("correction/results/") else {
            continue;
        };
        let Some(chapter_id) = rest.split('/').next() else {
            continue;
        };
        if chapter_id.is_empty()
            || chapter_id == "."
            || chapter_id == ".."
            || chapter_id.contains('\\')
        {
            continue;
        }
        chapters.insert(chapter_id.to_string());
    }
    for chapter_id in chapters {
        crate::correction::invalidate_synced_results(app_data_root, &project_id, &chapter_id);
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn sha256_text(text: &str) -> String {
    sha256_bytes(text.as_bytes())
}

fn version_matches(left: &ChapterVersion, right: &ChapterVersion) -> bool {
    left.object_generation == right.object_generation
        && left.revision == right.revision
        && left.content_sha256 == right.content_sha256
}

fn file_sha256(path: &Path) -> crate::error::Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(sha256_bytes(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// 从 workspace-relative 路径解析章节身份。
fn chapter_key_from_workspace_path(path: &Path) -> Option<ChapterKey> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    let volumes = components.iter().position(|part| *part == "volumes")?;
    if components.get(volumes + 2) != Some(&"chapters") {
        return None;
    }
    let volume_id = *components.get(volumes + 1)?;
    let chapter_id = *components.get(volumes + 3)?;
    let project_id = if volumes >= 2 && components[volumes - 2] == "projects" {
        components[volumes - 1]
    } else {
        components.first().copied().unwrap_or_default()
    };
    Some(ChapterKey::new(
        project_id, volume_id, chapter_id,
    ))
}

/// 从 target-relative 的章节 action 路径解析章节身份。
///
/// 支持 `volumes/<v>/chapters/<c>/<file>` 与
/// `projects/<pid>/volumes/<v>/chapters/<c>/<file>` 两种形态；无项目前缀时用
/// 当前 target 的默认 project id 补齐，保证与章节协调器生成的身份一致。
fn chapter_key_from_target_action(path: &Path, default_project_id: &str) -> Option<ChapterKey> {
    let parts: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    let (project_id, offset) = if parts.len() >= 2 && parts[0] == "projects" {
        (parts[1].clone(), 2)
    } else {
        (default_project_id.to_string(), 0)
    };
    if parts.len() != offset + 5 {
        return None;
    }
    if parts[offset] != "volumes" || parts[offset + 2] != "chapters" {
        return None;
    }
    if parts[offset + 4] != "chapter.md" && parts[offset + 4] != "chapter.meta.json" {
        return None;
    }
    Some(ChapterKey::new(
        project_id,
        parts[offset + 1].clone(),
        parts[offset + 3].clone(),
    ))
}

/// 提交前记录的已发布对象身份。路径是 workspace-relative；`deleted` 表示这条
/// 身份来自删除决议。
#[derive(Debug, Clone)]
pub(crate) struct CommittedPath {
    pub path: PathBuf,
    pub deleted: bool,
}

/// 统一对象比较产出的提交：内容、对象索引与已确认基线一次 durable 决议。
pub(crate) fn apply_baseline_updates(
    state: &mut crate::sync::types::SyncState,
    updates: &[crate::sync::staging::BaselineUpdate],
) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    for update in updates {
        let path = update.rel_path.to_string_lossy().replace('\\', "/");
        match &update.content_hash {
            Some(hash) => {
                state.known_files.insert(path.clone(), hash.clone());
                state.known_files_updated_at.insert(path.clone(), now_ms);
                state.deleted_files.remove(&path);
                state.conflicted_files.remove(&path);
                state.pending_take_remote.remove(&path);
                state.tombstones.retain(|tombstone| tombstone.original_path != path);
            }
            None => {
                state.known_files.remove(&path);
                state.known_files_updated_at.remove(&path);
                state.deleted_files.insert(path.clone());
                state.conflicted_files.remove(&path);
                state.pending_take_remote.remove(&path);
                state
                    .tombstones
                    .retain(|tombstone| tombstone.original_path != path);
            }
        }
    }
}

/// Rebase locally stored text anchors when an incoming chapter body is applied
/// without an incoming record for that anchor. The rebased anchor files join
/// the same durable decision as the body so a crash cannot leave new text with
/// stale offsets. Ambiguous, missing, or invalid UTF-8 inputs are left alone.
///
/// 每个 anchor 动作携带它依赖的章节决定：只有对应章节正文真的发布成功，
/// anchor 才允许落盘，避免用未应用的远端正文重排本地锚点。
fn rebase_chapter_anchors(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
) -> crate::error::Result<Vec<(crate::sync::staging::CommitAction, ChapterKey)>> {
    use crate::sync::staging::CommitAction;
    use std::collections::HashSet;

    let mut handled_chapters = HashSet::new();
    let mut staged_anchor_paths = HashSet::new();
    let mut rebased_anchors = Vec::new();
    let mut newest_existing_version = i64::MIN;
    for action in content_actions {
        let CommitAction::Apply { rel_path, content } = action else {
            continue;
        };
        let Some((project_namespace, project_relative, project_id)) =
            project_relative_sync_path(live_root, rel_path)
        else {
            continue;
        };
        let Some((volume_id, chapter_id)) = chapter_id_for_body_path(&project_relative) else {
            continue;
        };
        let chapter_key = (project_namespace.clone(), chapter_id.to_string());
        if !handled_chapters.insert(chapter_key) {
            continue;
        }
        let Ok(incoming_text) = std::str::from_utf8(content) else {
            continue;
        };

        let project_root = live_root.join(&project_namespace);
        let store = crate::annotations::AnnotationStore::new(&project_root, &project_id);
        for anchor in store.list_text_anchors_for_chapter(chapter_id)? {
            if anchor.project_id != project_id || !is_safe_annotation_id(&anchor.anchor_id) {
                continue;
            }
            let anchor_relative = PathBuf::from("annotations")
                .join("anchors")
                .join(format!("{}.json", anchor.anchor_id));
            let explicitly_changed = content_actions.iter().any(|candidate| {
                let candidate_path = match candidate {
                    CommitAction::Apply { rel_path, .. } | CommitAction::Delete { rel_path } => {
                        rel_path
                    }
                };
                project_relative_sync_path(live_root, candidate_path).is_some_and(
                    |(namespace, relative, _)| {
                        namespace == project_namespace && relative == anchor_relative
                    },
                )
            });
            if explicitly_changed
                || !staged_anchor_paths
                    .insert((project_namespace.clone(), anchor.anchor_id.clone()))
            {
                continue;
            }

            let Some(rebased) =
                crate::annotations::ranges::rebase_text_anchor(&anchor, incoming_text)?
            else {
                continue;
            };
            if rebased == anchor {
                continue;
            }
            newest_existing_version = newest_existing_version.max(anchor.updated_at_ms);
            rebased_anchors.push((
                project_namespace.join(&anchor_relative),
                rebased,
                ChapterKey::new(project_id.clone(), volume_id, chapter_id),
            ));
        }
    }
    if rebased_anchors.is_empty() {
        return Ok(Vec::new());
    }
    let updated_at_ms = chrono::Utc::now()
        .timestamp_millis()
        .max(newest_existing_version.saturating_add(1));
    let mut actions = Vec::with_capacity(rebased_anchors.len());
    for (rel_path, mut anchor, key) in rebased_anchors {
        anchor.updated_at_ms = updated_at_ms;
        actions.push((
            CommitAction::Apply {
                rel_path,
                content: serde_json::to_vec_pretty(&anchor)?,
            },
            key,
        ));
    }
    Ok(actions)
}

fn project_relative_sync_path(
    live_root: &Path,
    rel_path: &Path,
) -> Option<(PathBuf, PathBuf, String)> {
    use std::path::Component;

    let components = rel_path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if components.is_empty() {
        return None;
    }

    if components.len() >= 3 && components[0] == "projects" {
        let project_id = components[1].clone();
        let namespace = PathBuf::from("projects").join(&project_id);
        let project_relative = components[2..].iter().collect::<PathBuf>();
        return Some((namespace, project_relative, project_id));
    }

    let project_id = live_root.file_name()?.to_str()?.to_string();
    Some((PathBuf::new(), components.iter().collect(), project_id))
}

fn chapter_id_for_body_path(project_relative: &Path) -> Option<(&str, &str)> {
    let components = project_relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    match components.as_slice() {
        ["volumes", volume_id, "chapters", chapter_id, "chapter.md"] => {
            Some((volume_id, chapter_id))
        }
        _ => None,
    }
}

fn is_safe_annotation_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// staging manifest 中章节正文的来源设备与原始发生时间。事件 ID 和回执必须
/// 复用这份事实，不能在提交时重新取本机时间或空设备。
type ChapterSourceMap = std::collections::HashMap<(String, String, String), (String, i64)>;

fn staging_chapter_sources(
    run: &crate::sync::staging::StagingRun,
    default_project_id: Option<&str>,
) -> ChapterSourceMap {
    // 清单权威在 v2 store（staging root 下）；单文件只是 prepare 时物化的
    // 只读视图，合并后可能已过期。先读 store，没有 root 才回退单文件。
    let staging_root = run.staging_root();
    let manifest =
        crate::sync::lww::ManifestStore::new(&staging_root, crate::sync::lww::SYNC_MANIFEST_PATH)
            .load_all()
            .ok()
            .flatten()
            .or_else(|| {
                let text = std::fs::read_to_string(
                    staging_root.join(crate::sync::lww::SYNC_MANIFEST_PATH),
                )
                .ok()?;
                serde_json::from_str::<crate::sync::types::SyncManifest>(&text).ok()
            });
    let Some(manifest) = manifest else {
        return ChapterSourceMap::new();
    };
    let mut out = ChapterSourceMap::new();
    for record in manifest.files {
        if record.device_id.is_empty() {
            continue;
        }
        let Some(key) = chapter_key_from_workspace_path(Path::new(&record.path)) else {
            continue;
        };
        if !record.path.ends_with("chapter.md") {
            continue;
        }
        let project_id = if key.project_id.is_empty()
            || Path::new(&record.path)
                .components()
                .next()
                .is_some_and(|part| part.as_os_str() == "volumes")
        {
            default_project_id.unwrap_or_default().to_string()
        } else {
            key.project_id
        };
        out.entry((project_id, key.volume_id, key.chapter_id))
            .or_insert((record.device_id, record.updated_at_ms));
    }
    out
}

fn sync_chapter_event_id(
    run_id: &str,
    key: &crate::chapter::ChapterKey,
    mutation: crate::chapter::ChapterMutationKind,
) -> String {
    let suffix = match mutation {
        crate::chapter::ChapterMutationKind::Create => "create",
        crate::chapter::ChapterMutationKind::Delete => "delete",
        crate::chapter::ChapterMutationKind::SyncMerge => "sync_merge",
        crate::chapter::ChapterMutationKind::Replace => "replace",
        crate::chapter::ChapterMutationKind::Clear => "clear",
        crate::chapter::ChapterMutationKind::EditBatch => "edit_batch",
        crate::chapter::ChapterMutationKind::Restore => "restore",
    };
    format!(
        "sync:{run_id}:{}/{}/{}:{suffix}",
        key.project_id, key.volume_id, key.chapter_id
    )
}

/// 每章 journal 的确定性事务 ID：同一 target 的同一轮提交重复恢复时会复用
/// 同一条章节 journal，不会生成第二份决议。
fn sync_chapter_transaction_id(run_id: &str, key: &ChapterKey) -> String {
    let identity = sha256_text(&format!(
        "{}/{}/{}",
        key.project_id, key.volume_id, key.chapter_id
    ));
    let short = identity.get(..16).unwrap_or(identity.as_str());
    format!("sync-commit-{run_id}-{short}")
}

fn chapter_project_root(root: &Path, key: &crate::chapter::ChapterKey) -> PathBuf {
    if root.join("projects").is_dir() {
        root.join("projects").join(&key.project_id)
    } else {
        root.to_path_buf()
    }
}

fn chapter_target_rel_path(
    target_root: &Path,
    key: &crate::chapter::ChapterKey,
    meta: bool,
) -> PathBuf {
    let relative = if meta {
        PathBuf::from("volumes")
            .join(&key.volume_id)
            .join("chapters")
            .join(&key.chapter_id)
            .join("chapter.meta.json")
    } else {
        PathBuf::from("volumes")
            .join(&key.volume_id)
            .join("chapters")
            .join(&key.chapter_id)
            .join("chapter.md")
    };
    if target_root.join("projects").is_dir() {
        PathBuf::from("projects")
            .join(&key.project_id)
            .join(relative)
    } else {
        relative
    }
}

// ---------------------------------------------------------------------------
// durable commit 决议
// ---------------------------------------------------------------------------

/// 决议生命周期。`Decided` 是唯一决定；发布完所有条目后写 `Completed` 并清理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SyncCommitPhase {
    Decided,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncCommitTargetIdentity {
    kind: String,
    project_id: Option<String>,
    app_group: Option<AppDataGroup>,
    live_root: String,
}

/// 章节正文的最终决定。
///
/// 文本字节用材料 blob 引用，避免 manifest 本身随正文膨胀；`before`/`after`
/// 是提交时的真实版本，回执直接由这份决定推导，不再读盘猜。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncChapterDecision {
    key: ChapterKey,
    transaction_id: String,
    mutation: ChapterMutationKind,
    /// 发布后的正文内容（删除时为空）。
    body_blob: Option<String>,
    /// 同步证明事实里的 before/after 文本；Meta-only 决定没有证明事实。
    before_text_blob: Option<String>,
    after_text_blob: Option<String>,
    /// `None` 表示提交前章节不存在（Create）。
    before: Option<ChapterVersion>,
    /// 交给章节 journal 做 CAS 的期望版本。删除时来自共同基线（本地在同步后
    /// 又编辑过就会在 journal 里变成冲突），其它情况等于 `before`。
    expected: Option<ChapterVersion>,
    after: ChapterVersion,
    /// Create 用的初始 meta 模板（含固定 object_generation）。
    meta_template: Option<Chapter>,
    meta_title: Option<String>,
    meta_order: Option<i32>,
    event_id: Option<String>,
    source_device_id: String,
    occurred_at_ms: i64,
    deleted: bool,
    /// 删除提交的真实 app data root 与固定 trash 路径。
    app_data_root: Option<String>,
    trash_path: Option<String>,
    /// 冲突材料用的 base 正文。
    base_content_blob: Option<String>,
    write_body: bool,
    write_meta: bool,
}

/// 单个非章节文件的最终决定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncFileDecision {
    /// 相对 target live root。
    rel_path: String,
    /// `None` 表示删除。
    blob: Option<String>,
    before_sha256: Option<String>,
    after_sha256: Option<String>,
    deleted: bool,
    /// 依赖的章节决定（annotation rebase 等）：只有该章正文发布成功，这个
    /// 文件才允许落盘。
    #[serde(default)]
    chapter_dependency: Option<ChapterKey>,
}

/// 一个 sync target 的单一持久决议。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncCommitManifest {
    schema_version: u32,
    phase: SyncCommitPhase,
    target: SyncCommitTargetIdentity,
    run_id: String,
    operation_id: String,
    decided_at_ms: i64,
    chapters: Vec<SyncChapterDecision>,
    files: Vec<SyncFileDecision>,
    /// 本次决定产生的最终冲突记录（canonical）；发布状态由 `conflicts_changed`
    /// 与 `conflicts.json` 文件条目共同判定。
    conflicts: Vec<SyncConflict>,
    conflicts_changed: bool,
}

fn decision_dir(live_root: &Path, group: Option<&AppDataGroup>) -> PathBuf {
    match group {
        Some(group) => live_root
            .join("app-meta/sync/groups")
            .join(group.storage_key())
            .join("commit-decision"),
        None => live_root.join(SYNC_COMMIT_DECISION_DIR),
    }
}

fn conflict_records_rel_path(group: Option<&AppDataGroup>) -> String {
    group
        .map(AppDataGroup::local_conflicts_path)
        .unwrap_or_else(|| "app-meta/sync/conflicts.json".to_string())
}

fn state_rel_path(group: Option<&AppDataGroup>) -> String {
    group
        .map(AppDataGroup::local_state_path)
        .unwrap_or_else(|| "app-meta/sync/state.local.json".to_string())
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join(SYNC_COMMIT_MANIFEST_FILE)
}

/// 决议里记录的 live root 必须就是恢复入口以为的那个根；避免损坏/搬移的
/// manifest 把发布写到别的 target。
fn same_directory(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn write_manifest(dir: &Path, manifest: &SyncCommitManifest) -> crate::error::Result<()> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?;
    crate::storage::atomic_write_bytes(&manifest_path(dir), json.as_bytes())?;
    Ok(())
}

fn load_manifest(dir: &Path) -> crate::error::Result<Option<SyncCommitManifest>> {
    let path = manifest_path(dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let manifest: SyncCommitManifest = serde_json::from_str(&raw).map_err(|error| {
        crate::Error::Other(format!(
            "sync commit decision {} is corrupt: {error}",
            path.display()
        ))
    })?;
    if manifest.schema_version != SYNC_COMMIT_SCHEMA_VERSION {
        return Err(crate::Error::Other(format!(
            "sync commit decision {} has unsupported schema version {}",
            path.display(),
            manifest.schema_version
        )));
    }
    Ok(Some(manifest))
}

/// `Completed` 决议只需要清理；`Decided` 决议才会继续发布。
fn remove_completed_decision(dir: &Path, manifest: &SyncCommitManifest) {
    if manifest.phase != SyncCommitPhase::Completed {
        return;
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "[sync] failed to clean completed commit decision {}: {error}",
            dir.display()
        ),
    }
}

fn finish_manifest(dir: &Path, manifest: &SyncCommitManifest) {
    let mut completed = manifest.clone();
    completed.phase = SyncCommitPhase::Completed;
    if let Err(error) = write_manifest(dir, &completed) {
        // 所有条目已发布；清理失败不影响业务结果，下次恢复按现有文件状态
        // 重新确认并清理，绝不能把已成功的写盘报成失败。
        log::warn!(
            "[sync] failed to finalize commit decision {}: {error} (next recovery retries)",
            dir.display()
        );
        return;
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "[sync] failed to clean commit decision {}: {error}",
            dir.display()
        ),
    }
}

fn write_material(dir: &Path, bytes: &[u8]) -> crate::error::Result<String> {
    let materials = dir.join(SYNC_COMMIT_MATERIALS_DIR);
    let name = format!("blob-{}", sha256_bytes(bytes));
    let path = materials.join(&name);
    match std::fs::read(&path) {
        Ok(existing) if existing == bytes => return Ok(name),
        Ok(_) => {
            return Err(crate::Error::Other(format!(
                "sync commit material digest collision at {}",
                path.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    crate::storage::atomic_write_bytes(&path, bytes)?;
    Ok(name)
}

fn read_material(dir: &Path, name: &str) -> crate::error::Result<Vec<u8>> {
    // 材料名必须是本模块生成的 `blob-<sha256>`；先校验再拼路径，损坏/伪造
    // manifest 不能借材料名穿越决议目录。
    let Some(expected) = name.strip_prefix("blob-") else {
        return Err(crate::Error::Other(format!(
            "sync commit material has invalid name: {name}"
        )));
    };
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(crate::Error::Other(format!(
            "sync commit material has invalid name: {name}"
        )));
    }
    let path = dir.join(SYNC_COMMIT_MATERIALS_DIR).join(name);
    let bytes = std::fs::read(&path).map_err(|error| {
        crate::Error::Other(format!(
            "sync commit material {} is unavailable: {error}",
            path.display()
        ))
    })?;
    if sha256_bytes(&bytes) != expected {
        return Err(crate::Error::Other(format!(
            "sync commit material {} does not match its digest",
            path.display()
        )));
    }
    Ok(bytes)
}

fn read_material_text(dir: &Path, name: &str) -> crate::error::Result<String> {
    let bytes = read_material(dir, name)?;
    String::from_utf8(bytes).map_err(|error| {
        crate::Error::Other(format!(
            "sync commit material {name} is not UTF-8: {error}"
        ))
    })
}

/// 规划阶段先写材料、后写 manifest；manifest 不存在说明这次规划没有完成，
/// 残留材料可以直接清掉。
fn reset_orphan_decision_dir(dir: &Path) {
    if manifest_path(dir).exists() {
        return;
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "[sync] failed to clean orphan commit decision {}: {error}",
            dir.display()
        ),
    }
}

fn decision_blob(dir: &Path, blob: Option<&str>) -> crate::error::Result<Option<String>> {
    match blob {
        Some(blob) => read_material_text(dir, blob).map(Some),
        None => Ok(None),
    }
}

fn chapter_change_receipt(
    decision: &SyncChapterDecision,
    operation_id: &str,
) -> ChapterChangeReceipt {
    let content_changed = decision.deleted
        || decision.mutation == ChapterMutationKind::Create
        || match decision.before.as_ref() {
            Some(before) => before.content_sha256 != decision.after.content_sha256,
            None => true,
        };
    ChapterChangeReceipt {
        key: decision.key.clone(),
        // Create 的 `before` 只是决定里用于 CAS/after 复算的合成版本；
        // 公开回执沿用旧语义：提交前文件不存在 → None。
        before: if decision.mutation == ChapterMutationKind::Create {
            None
        } else {
            decision.before.clone()
        },
        after: if decision.deleted {
            None
        } else {
            Some(decision.after.clone())
        },
        content_changed,
        deleted: decision.deleted,
        operation_id: operation_id.to_string(),
    }
}

fn chapter_sync_receipt(
    decision: &SyncChapterDecision,
    dir: &Path,
) -> crate::error::Result<ChapterSyncCommitReceipt> {
    Ok(ChapterSyncCommitReceipt {
        transaction_id: decision.transaction_id.clone(),
        key: decision.key.clone(),
        event_id: decision.event_id.clone(),
        source_device_id: decision.source_device_id.clone(),
        occurred_at_ms: decision.occurred_at_ms,
        before_text: decision_blob(dir, decision.before_text_blob.as_deref())?,
        after_text: decision_blob(dir, decision.after_text_blob.as_deref())?,
        deleted: decision.deleted,
    })
}

fn chapter_written_paths(
    live_root: &Path,
    decision: &SyncChapterDecision,
) -> Vec<(PathBuf, bool)> {
    let mut out = Vec::new();
    if decision.write_body {
        out.push((
            chapter_target_rel_path(live_root, &decision.key, false),
            decision.deleted,
        ));
    }
    if decision.write_meta {
        out.push((
            chapter_target_rel_path(live_root, &decision.key, true),
            decision.deleted,
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// 决定规划
// ---------------------------------------------------------------------------

/// 把本 target 的章节正文 action 统一规划成 durable 章节决定。
///
/// 这里只读取 live 当前版本（在 workspace 写锁内）并写材料，不碰 live、不建
/// journal。真正发布在 manifest durable 之后由 [`publish_manifest`] 逐章完成。
#[allow(
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_lines
)]
fn plan_chapter_decisions(
    run: &StagingRun,
    guard: &crate::storage::WorkspaceWriteLock,
    content_actions: &[CommitAction],
    chapter_sources: &ChapterSourceMap,
    decision_dir: &Path,
) -> crate::error::Result<(Vec<SyncChapterDecision>, HashSet<ChapterKey>)> {
    use super::chapter_mutation_plan::{plan_synced_chapter_mutations, SyncedChapterMutation};

    let live_root = run.target_live_root();
    let base_root = run.base_root();
    let app_data_root = run.app_data_root();
    let run_id = run.run_id();
    let project_id = live_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    if project_id.is_empty() {
        return Err(crate::error::Error::Other(
            "sync chapter commit cannot derive project id from live root".to_string(),
        ));
    }

    let mut decisions = Vec::new();
    let mut handled = HashSet::new();
    for change in plan_synced_chapter_mutations(&project_id, content_actions)? {
        let chapter_root = chapter_project_root(live_root, &change.key);
        let base_chapter_root = chapter_project_root(&base_root, &change.key);
        handled.insert(change.key.clone());
        let (source_device_id, occurred_at_ms) = chapter_sources
            .get(&(
                change.key.project_id.clone(),
                change.key.volume_id.clone(),
                change.key.chapter_id.clone(),
            ))
            .cloned()
            .unwrap_or_else(|| (String::new(), chrono::Utc::now().timestamp_millis()));
        let transaction_id = sync_chapter_transaction_id(run_id, &change.key);

        if change.mutation == SyncedChapterMutation::Delete {
            let base = crate::chapter::commit::read_chapter_state(
                &base_chapter_root,
                &change.key.volume_id,
                &change.key.chapter_id,
            );
            let live = crate::chapter::commit::read_chapter_state_recovering(
                &chapter_root,
                guard,
                &change.key.volume_id,
                &change.key.chapter_id,
            );
            match (base, live) {
                (
                    Err(crate::error::Error::ChapterNotFound),
                    Err(crate::error::Error::ChapterNotFound),
                ) => {
                    // base 与 live 都不存在：删除已幂等完成，不产生新决定。
                    continue;
                }
                (Err(crate::error::Error::ChapterNotFound), Ok(_)) => {
                    return Err(crate::error::Error::Other(format!(
                        "sync delete chapter created after base: {}/{}",
                        change.key.volume_id, change.key.chapter_id
                    )));
                }
                (Ok((_, base_text, base_version)), Ok((_, live_text, live_version))) => {
                    let expected = if base_version.object_generation.is_empty()
                        || base_version.content_sha256.is_empty()
                    {
                        if base_text != live_text || base_version.revision != live_version.revision
                        {
                            return Err(crate::error::Error::Other(format!(
                                "sync delete legacy base changed: {}/{}",
                                change.key.volume_id, change.key.chapter_id
                            )));
                        }
                        live_version.clone()
                    } else {
                        base_version
                    };
                    let after = ChapterVersion {
                        object_generation: live_version.object_generation.clone(),
                        revision: live_version.revision.saturating_add(1),
                        content_sha256: sha256_text(""),
                    };
                    let before_text_blob = Some(write_material(
                        decision_dir,
                        live_text.as_bytes(),
                    )?);
                    let trash_path = app_data_root
                        .join("sync/trash")
                        .join(format!(
                            "{}_{}_{}",
                            chrono::Utc::now().timestamp_millis(),
                            uuid::Uuid::new_v4(),
                            change.key.chapter_id
                        ))
                        .to_string_lossy()
                        .into_owned();
                    decisions.push(SyncChapterDecision {
                        key: change.key.clone(),
                        transaction_id,
                        mutation: ChapterMutationKind::Delete,
                        body_blob: None,
                        before_text_blob,
                        after_text_blob: None,
                        before: Some(live_version),
                        expected: Some(expected),
                        after,
                        meta_template: None,
                        meta_title: None,
                        meta_order: None,
                        event_id: Some(sync_chapter_event_id(
                            run_id,
                            &change.key,
                            ChapterMutationKind::Delete,
                        )),
                        source_device_id,
                        occurred_at_ms,
                        deleted: true,
                        app_data_root: Some(app_data_root.to_string_lossy().into_owned()),
                        trash_path: Some(trash_path),
                        base_content_blob: None,
                        write_body: true,
                        write_meta: true,
                    });
                }
                (Ok(_), Err(crate::error::Error::ChapterNotFound)) => {
                    // live 已被删除（可能是外部）；不再重复发布。
                    continue;
                }
                (Err(error), _) => return Err(error),
                (_, Err(error)) => return Err(error),
            }
            continue;
        }

        let current = crate::chapter::commit::read_chapter_state_recovering(
            &chapter_root,
            guard,
            &change.key.volume_id,
            &change.key.chapter_id,
        );
        match current {
            Ok((meta, current_text, current_version)) => {
                let body = match change.mutation {
                    SyncedChapterMutation::Meta => current_text.clone(),
                    _ => change
                        .body
                        .clone()
                        .unwrap_or_else(|| current_text.clone()),
                };
                let content_changed = body != current_text;
                let meta_title = change.meta.as_ref().map(|meta| meta.title.clone());
                let meta_order = change.meta.as_ref().map(|meta| meta.order);
                let meta_changed = meta_title
                    .as_ref()
                    .is_some_and(|title| title != &meta.title)
                    || meta_order.is_some_and(|order| order != meta.order);
                let event_id = if change.mutation == SyncedChapterMutation::Meta {
                    None
                } else {
                    Some(sync_chapter_event_id(
                        run_id,
                        &change.key,
                        ChapterMutationKind::SyncMerge,
                    ))
                };
                let after = ChapterVersion {
                    object_generation: current_version.object_generation.clone(),
                    revision: if content_changed || meta_changed {
                        current_version.revision.saturating_add(1)
                    } else {
                        current_version.revision
                    },
                    content_sha256: sha256_text(&body),
                };
                let before_text_blob = if event_id.is_some() {
                    Some(write_material(decision_dir, current_text.as_bytes())?)
                } else {
                    None
                };
                let after_text_blob = if event_id.is_some() {
                    Some(write_material(decision_dir, body.as_bytes())?)
                } else {
                    None
                };
                decisions.push(SyncChapterDecision {
                    key: change.key.clone(),
                    transaction_id,
                    mutation: ChapterMutationKind::SyncMerge,
                    body_blob: Some(write_material(decision_dir, body.as_bytes())?),
                    before_text_blob,
                    after_text_blob,
                    before: Some(current_version.clone()),
                    expected: Some(current_version),
                    after,
                    meta_template: None,
                    meta_title,
                    meta_order,
                    event_id,
                    source_device_id,
                    occurred_at_ms,
                    deleted: false,
                    app_data_root: None,
                    trash_path: None,
                    base_content_blob: Some(write_material(
                        decision_dir,
                        current_text.as_bytes(),
                    )?),
                    write_body: content_changed,
                    write_meta: content_changed || meta_changed,
                });
            }
            Err(crate::error::Error::ChapterNotFound) => {
                if change.mutation == SyncedChapterMutation::Meta {
                    // 只有 meta 的新章节无法证明正文来源；不能退回通用
                    // SaveTransaction 留下无正文的章节。
                    return Err(crate::error::Error::Other(format!(
                        "sync chapter meta without body: volumes/{}/chapters/{}",
                        change.key.volume_id, change.key.chapter_id
                    )));
                }
                let Some(mut meta_template) = change.meta.clone() else {
                    return Err(crate::error::Error::Other(format!(
                        "sync chapter create without meta: volumes/{}/chapters/{}",
                        change.key.volume_id, change.key.chapter_id
                    )));
                };
                let Some(body) = change.body.clone() else {
                    return Err(crate::error::Error::Other(format!(
                        "sync chapter create without body: volumes/{}/chapters/{}",
                        change.key.volume_id, change.key.chapter_id
                    )));
                };
                if meta_template.object_generation.is_empty() {
                    // journal 会用模板 generation 作为本次创建的身份；这里先在
                    // 决定里固定，恢复时 after 版本才可复算。
                    meta_template.object_generation = uuid::Uuid::new_v4().to_string();
                }
                let before = ChapterVersion {
                    object_generation: meta_template.object_generation.clone(),
                    revision: 0,
                    content_sha256: sha256_text(""),
                };
                let after = ChapterVersion {
                    object_generation: meta_template.object_generation.clone(),
                    revision: if body.is_empty() { 0 } else { 1 },
                    content_sha256: sha256_text(&body),
                };
                decisions.push(SyncChapterDecision {
                    key: change.key.clone(),
                    transaction_id,
                    mutation: ChapterMutationKind::Create,
                    body_blob: Some(write_material(decision_dir, body.as_bytes())?),
                    before_text_blob: None,
                    after_text_blob: Some(write_material(decision_dir, body.as_bytes())?),
                    before: Some(before),
                    expected: None,
                    after,
                    meta_template: Some(meta_template),
                    meta_title: None,
                    meta_order: None,
                    event_id: Some(sync_chapter_event_id(
                        run_id,
                        &change.key,
                        ChapterMutationKind::Create,
                    )),
                    source_device_id,
                    occurred_at_ms,
                    deleted: false,
                    app_data_root: None,
                    trash_path: None,
                    base_content_blob: None,
                    write_body: true,
                    write_meta: true,
                });
            }
            Err(error) => return Err(error),
        }
    }
    Ok((decisions, handled))
}

/// 冲突材料的最终决定：先冻结远端快照路径与字节，再把 canonical 冲突记录
/// 作为 state/conflicts 文件决定的一部分发布，不再另外追加冲突写事务。
struct ConflictDecision {
    /// 已按 resolved 过滤、但尚未合并本次 records 的 canonical 列表。
    canonical: Vec<SyncConflict>,
    /// target.result.conflicts 中属于该 group 的传输冲突。
    existing: Vec<SyncConflict>,
    /// 本轮 staging 冲突（远端快照路径已冻结）。
    staging: Vec<SyncConflict>,
    /// conflicts.json 是否需要重写。
    changed: bool,
}

fn conflict_snapshot_rel(rel_path: &Path, decided_at_ms: i64) -> crate::error::Result<String> {
    let raw = rel_path.to_string_lossy().replace('\\', "/");
    let validated = crate::sync::path::ValidatedSyncPath::new(&raw)?;
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(decided_at_ms)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y%m%d-%H%M%S")
        .to_string();
    let filename = validated
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    let parent = validated
        .as_str()
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default();
    let conflict_name = format!("{filename}.remote-conflict-{timestamp}");
    let snapshot = if parent.is_empty() {
        conflict_name
    } else {
        format!("{parent}/{conflict_name}")
    };
    crate::sync::path::ValidatedSyncPath::new(&snapshot)?;
    Ok(snapshot)
}

#[allow(clippy::too_many_lines)]
fn plan_conflict_decisions(
    run: &StagingRun,
    plan: &CommitPlan,
    target: &TargetSyncResult,
    decision_dir: &Path,
    decided_at_ms: i64,
    files: &mut Vec<SyncFileDecision>,
    file_index: &mut HashMap<String, usize>,
) -> crate::error::Result<ConflictDecision> {
    let live_root = run.target_live_root();
    let mut conflicts = match run.app_group() {
        Some(group) => crate::sync::conflict::load_conflicts_json_for_group(live_root, group)?,
        None => crate::sync::conflict::load_conflicts_json(live_root)?,
    };
    let resolved: HashSet<String> = plan
        .confirmations
        .iter()
        .map(|update| update.rel_path.to_string_lossy().replace('\\', "/"))
        .collect();
    let before = conflicts.len();
    conflicts.retain(|conflict| !resolved.contains(&conflict.local_path));
    let resolved_removed = conflicts.len() != before;

    let mut existing = Vec::new();
    let mut staging_records = Vec::new();
    if !plan.conflict.is_empty() {
        let group = run.app_group();
        for conflict in &target.result.conflicts {
            if resolved.contains(&conflict.local_path) {
                // 本轮已确认远端版本的路径，不再把旧冲突记录写回去。
                continue;
            }
            if group.is_some_and(|group| !group.owns_path(&conflict.local_path)) {
                continue;
            }
            existing.push(conflict.clone());
        }
        for staging in &plan.conflict {
            let mut staging = staging.clone();
            if let Some(group) = group {
                let path = staging.rel_path.to_string_lossy().replace('\\', "/");
                if !group.owns_path(&path) {
                    return Err(crate::error::Error::Other(format!(
                        "record_staging_conflicts: path '{}' is outside App data group '{}'",
                        path,
                        group.id()
                    )));
                }
            }
            if staging.kind == SyncConflictKind::BothChanged
                && staging.remote_snapshot_path.is_none()
            {
                let incoming_path = run.staging_root().join(&staging.rel_path);
                match std::fs::read(&incoming_path) {
                    Ok(incoming) => {
                        let snapshot_rel =
                            conflict_snapshot_rel(&staging.rel_path, decided_at_ms)?;
                        push_apply_file_decision(
                            decision_dir,
                            live_root,
                            files,
                            file_index,
                            snapshot_rel.clone(),
                            &incoming,
                        )?;
                        staging.remote_snapshot_path = Some(snapshot_rel);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        // BothChanged 理论上应有 incoming；缺失时保留 None 兜底。
                    }
                    Err(error) => {
                        return Err(crate::error::Error::Io(std::io::Error::other(format!(
                            "read conflict snapshot {}: {error}",
                            incoming_path.display()
                        ))))
                    }
                }
            }
            let rel_unix = staging.rel_path.to_string_lossy().replace('\\', "/");
            let description = match staging.kind {
                SyncConflictKind::BothChanged => format!(
                    "three-way conflict: both local and remote changed {}",
                    staging.rel_path.display()
                ),
                SyncConflictKind::RemoteDeleted => format!(
                    "conflict: remote deleted {} but local modified",
                    staging.rel_path.display()
                ),
            };
            staging_records.push(SyncConflict {
                local_path: staging.rel_path.to_string_lossy().to_string(),
                remote_path: format!("{}/{}", target.remote_prefix, rel_unix),
                local_hash: staging.local_hash.clone(),
                remote_hash: staging.incoming_hash.clone(),
                base_hash: staging.base_hash.clone(),
                created_at: decided_at_ms / 1000,
                description,
                kind: staging.kind,
                remote_snapshot_path: staging.remote_snapshot_path.clone(),
                remote_graph_snapshot_dir: staging.remote_graph_snapshot_dir.clone(),
            });
        }
    }

    Ok(ConflictDecision {
        canonical: conflicts,
        changed: !plan.conflict.is_empty() || resolved_removed,
        existing,
        staging: staging_records,
    })
}

fn push_file_decision(
    files: &mut Vec<SyncFileDecision>,
    index: &mut HashMap<String, usize>,
    decision: SyncFileDecision,
) {
    match index.get(&decision.rel_path).copied() {
        Some(position) => files[position] = decision,
        None => {
            index.insert(decision.rel_path.clone(), files.len());
            files.push(decision);
        }
    }
}

fn push_apply_file_decision(
    decision_dir: &Path,
    live_root: &Path,
    files: &mut Vec<SyncFileDecision>,
    index: &mut HashMap<String, usize>,
    rel_path: String,
    bytes: &[u8],
) -> crate::error::Result<()> {
    let before_sha256 = file_sha256(&live_root.join(&rel_path))?;
    let blob = write_material(decision_dir, bytes)?;
    push_file_decision(
        files,
        index,
        SyncFileDecision {
            rel_path,
            blob: Some(blob),
            before_sha256,
            after_sha256: Some(sha256_bytes(bytes)),
            deleted: false,
            chapter_dependency: None,
        },
    );
    Ok(())
}

fn push_delete_file_decision(
    live_root: &Path,
    files: &mut Vec<SyncFileDecision>,
    index: &mut HashMap<String, usize>,
    rel_path: String,
) -> crate::error::Result<()> {
    let before_sha256 = file_sha256(&live_root.join(&rel_path))?;
    push_file_decision(
        files,
        index,
        SyncFileDecision {
            rel_path,
            blob: None,
            before_sha256,
            after_sha256: None,
            deleted: true,
            chapter_dependency: None,
        },
    );
    Ok(())
}

fn push_action_file_decision(
    decision_dir: &Path,
    live_root: &Path,
    files: &mut Vec<SyncFileDecision>,
    index: &mut HashMap<String, usize>,
    action: &CommitAction,
) -> crate::error::Result<()> {
    match action {
        CommitAction::Apply { rel_path, content } => push_apply_file_decision(
            decision_dir,
            live_root,
            files,
            index,
            rel_path.to_string_lossy().into_owned(),
            content,
        ),
        CommitAction::Delete { rel_path } => push_delete_file_decision(
            live_root,
            files,
            index,
            rel_path.to_string_lossy().into_owned(),
        ),
    }
}

/// annotation rebase 动作必须依赖对应章节正文真的发布成功；把章节身份写进
/// 文件决定，发布时再据此过滤。
fn push_anchor_file_decision(
    decision_dir: &Path,
    live_root: &Path,
    files: &mut Vec<SyncFileDecision>,
    index: &mut HashMap<String, usize>,
    action: &CommitAction,
    key: ChapterKey,
) -> crate::error::Result<()> {
    let (rel_path, content, deleted) = match action {
        CommitAction::Apply { rel_path, content } => (
            rel_path.to_string_lossy().into_owned(),
            Some(&content[..]),
            false,
        ),
        CommitAction::Delete { rel_path } => (rel_path.to_string_lossy().into_owned(), None, true),
    };
    let before_sha256 = file_sha256(&live_root.join(&rel_path))?;
    let (blob, after_sha256) = match content {
        Some(bytes) => (
            Some(write_material(decision_dir, bytes)?),
            Some(sha256_bytes(bytes)),
        ),
        None => (None, None),
    };
    push_file_decision(
        files,
        index,
        SyncFileDecision {
            rel_path,
            blob,
            before_sha256,
            after_sha256,
            deleted,
            chapter_dependency: Some(key),
        },
    );
    Ok(())
}

/// 把 baseline/结算/最终冲突记录一起冻结成 state 与 conflicts 文件决定。
fn build_state_file_decisions(
    run: &StagingRun,
    plan: &CommitPlan,
    conflict: &ConflictDecision,
    decision_dir: &Path,
    files: &mut Vec<SyncFileDecision>,
    file_index: &mut HashMap<String, usize>,
) -> crate::error::Result<Vec<SyncConflict>> {
    let live_root = run.target_live_root();
    let mut state: SyncState = match run.app_group() {
        Some(group) => crate::sync::SyncService::load_sync_state_read_only_for_group(
            live_root, group, None,
        )?,
        None => crate::sync::SyncService::load_sync_state_read_only(live_root, None)?,
    };
    apply_baseline_updates(&mut state, &plan.confirmations);
    // 与内容/索引同一次提交结算账本序号：NoOp/双删/被远端覆盖的事实
    // 不再留到下一轮被当成待同步变更。
    for fact in &plan.settlements {
        let current = state
            .confirmed_mutation_seq
            .get(&fact.path)
            .copied()
            .unwrap_or(0);
        if fact.seq > current {
            state
                .confirmed_mutation_seq
                .insert(fact.path.clone(), fact.seq);
        }
    }
    let resolved: HashSet<String> = plan
        .confirmations
        .iter()
        .map(|update| update.rel_path.to_string_lossy().replace('\\', "/"))
        .collect();
    if !resolved.is_empty() {
        state
            .conflicts
            .retain(|conflict| !resolved.contains(&conflict.local_path));
    }

    // canonical 与 state 镜像都按同一份 records upsert，避免冲突记录与
    // state.conflicts 分两次事务写、在崩溃后各自半套。
    let mut canonical = conflict.canonical.clone();
    for record in conflict.existing.iter().chain(conflict.staging.iter()) {
        crate::sync::conflict::upsert_conflict(
            &mut canonical,
            &mut state.conflicts,
            &mut state.conflicted_files,
            record.clone(),
        );
    }

    let state_rel = state_rel_path(run.app_group());
    let state_bytes = serde_json::to_vec_pretty(&state)
        .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?;
    push_apply_file_decision(
        decision_dir,
        live_root,
        files,
        file_index,
        state_rel,
        &state_bytes,
    )?;

    if conflict.changed {
        let conflicts_rel = conflict_records_rel_path(run.app_group());
        let conflicts_bytes = serde_json::to_vec_pretty(&canonical)
            .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?;
        push_apply_file_decision(
            decision_dir,
            live_root,
            files,
            file_index,
            conflicts_rel,
            &conflicts_bytes,
        )?;
    }

    // target 结果沿用旧语义：本 target 已有的传输冲突 + 本轮 staging 冲突，
    // 而不是把其它 target 的历史 canonical 记录重复塞进每个 target DTO。
    Ok(crate::sync::conflict::merge_sync_conflicts(
        &conflict.existing,
        &conflict.staging,
    ))
}

// ---------------------------------------------------------------------------
// 发布
// ---------------------------------------------------------------------------

enum ChapterPublish {
    Applied,
    Superseded(String),
}

#[allow(clippy::too_many_lines)]
fn publish_chapter_decision(
    live_root: &Path,
    guard: &crate::storage::WorkspaceWriteLock,
    decision_dir: &Path,
    decision: &SyncChapterDecision,
) -> crate::error::Result<ChapterPublish> {
    let key_segments = [
        decision.key.project_id.as_str(),
        decision.key.volume_id.as_str(),
        decision.key.chapter_id.as_str(),
    ];
    if key_segments.iter().any(|segment| {
        segment.is_empty()
            || *segment == "."
            || *segment == ".."
            || segment.contains('/')
            || segment.contains('\\')
    }) {
        return Ok(ChapterPublish::Superseded(
            "invalid chapter identity in sync commit decision".to_string(),
        ));
    }
    let project_root = chapter_project_root(live_root, &decision.key);
    let current = crate::chapter::commit::read_chapter_state_recovering(
        &project_root,
        guard,
        &decision.key.volume_id,
        &decision.key.chapter_id,
    );

    let publish = |request: ChapterCommitRequest| -> crate::error::Result<ChapterPublish> {
        match crate::chapter::commit::commit_chapter_with_guard(
            &project_root,
            Some(guard),
            request,
        )? {
            ChapterCommitOutcome::Committed(receipt) => {
                if version_matches(&receipt.after, &decision.after) {
                    Ok(ChapterPublish::Applied)
                } else {
                    Err(crate::error::Error::Other(format!(
                        "sync chapter commit recommitted a different version for {}/{}/{}: \
                         expected revision {}, actual {}",
                        decision.key.project_id,
                        decision.key.volume_id,
                        decision.key.chapter_id,
                        decision.after.revision,
                        receipt.after.revision
                    )))
                }
            }
            ChapterCommitOutcome::Conflict(conflict) => Ok(ChapterPublish::Superseded(format!(
                "chapter commit conflicted for {}/{}/{}: actual revision {}, dir={}",
                decision.key.project_id,
                decision.key.volume_id,
                decision.key.chapter_id,
                conflict.actual.revision,
                conflict.conflict_dir
            ))),
        }
    };

    match current {
        Ok((_, _, version)) => {
            if version_matches(&version, &decision.after) {
                return Ok(ChapterPublish::Applied);
            }
            if decision.deleted {
                let Some(before) = decision.before.as_ref() else {
                    return Ok(ChapterPublish::Superseded(
                        "delete decision has no before version".to_string(),
                    ));
                };
                if !version_matches(&version, before) {
                    return Ok(ChapterPublish::Superseded(format!(
                        "chapter changed before delete publish: current revision {}",
                        version.revision
                    )));
                }
                let mut request = ChapterCommitRequest::new(decision.key.clone(), "");
                let Some(expected) = decision.expected.clone() else {
                    return Ok(ChapterPublish::Superseded(
                        "delete decision has no expected version".to_string(),
                    ));
                };
                request.expected = Some(expected);
                request.mutation = ChapterMutationKind::Delete;
                request.source = crate::chapter::ChapterCommitSource::Sync;
                request.allow_empty_overwrite = true;
                request.transaction_id = Some(decision.transaction_id.clone());
                request.app_data_root = decision
                    .app_data_root
                    .as_deref()
                    .map(std::path::PathBuf::from);
                request.trash_path = decision
                    .trash_path
                    .as_deref()
                    .map(std::path::PathBuf::from);
                if decision.event_id.is_some() {
                    request
                        .sync_proof_facts
                        .push(decision_proof_fact(decision, decision_dir)?);
                }
                return publish(request);
            }
            let Some(before) = decision.before.as_ref() else {
                return Ok(ChapterPublish::Superseded(
                    "chapter appeared after a create decision".to_string(),
                ));
            };
            if !version_matches(&version, before) {
                return Ok(ChapterPublish::Superseded(format!(
                    "chapter changed before publish: current revision {}",
                    version.revision
                )));
            }
            let body = match decision.body_blob.as_deref() {
                Some(blob) => read_material_text(decision_dir, blob)?,
                None => String::new(),
            };
            let mut request = ChapterCommitRequest::new(decision.key.clone(), body);
            request.expected = decision.expected.clone();
            request.mutation = ChapterMutationKind::SyncMerge;
            request.source = crate::chapter::ChapterCommitSource::Sync;
            // 同步决定已经由远端/三方合并裁决过；空正文也是明确决定
            // （双方清空），不能在这里被本地空覆盖保护再次拦下。
            request.allow_empty_overwrite = true;
            request.transaction_id = Some(decision.transaction_id.clone());
            request.meta_title = decision.meta_title.clone();
            request.meta_order = decision.meta_order;
            request.base_content = decision_blob(decision_dir, decision.base_content_blob.as_deref())?;
            if decision.event_id.is_some() {
                request
                    .sync_proof_facts
                    .push(decision_proof_fact(decision, decision_dir)?);
            }
            publish(request)
        }
        Err(crate::error::Error::ChapterNotFound) => {
            if decision.deleted {
                return Ok(ChapterPublish::Applied);
            }
            if decision.mutation != ChapterMutationKind::Create {
                return Ok(ChapterPublish::Superseded(
                    "chapter deleted after a replace decision".to_string(),
                ));
            }
            let Some(meta_template) = decision.meta_template.clone() else {
                return Ok(ChapterPublish::Superseded(
                    "create decision has no meta template".to_string(),
                ));
            };
            let body = match decision.body_blob.as_deref() {
                Some(blob) => read_material_text(decision_dir, blob)?,
                None => String::new(),
            };
            let mut request = ChapterCommitRequest::new(decision.key.clone(), body);
            request.mutation = ChapterMutationKind::Create;
            request.source = crate::chapter::ChapterCommitSource::Sync;
            request.allow_empty_overwrite = true;
            request.meta_template = Some(meta_template);
            request.transaction_id = Some(decision.transaction_id.clone());
            if decision.event_id.is_some() {
                request
                    .sync_proof_facts
                    .push(decision_proof_fact(decision, decision_dir)?);
            }
            publish(request)
        }
        Err(error) => Err(error),
    }
}

fn decision_proof_fact(
    decision: &SyncChapterDecision,
    decision_dir: &Path,
) -> crate::error::Result<ChapterSyncProofFact> {
    let Some(event_id) = decision.event_id.as_ref() else {
        return Err(crate::error::Error::Other(
            "sync chapter decision has no proof event id".to_string(),
        ));
    };
    Ok(ChapterSyncProofFact {
        event_id: event_id.clone(),
        key: decision.key.clone(),
        before_text: decision_blob(decision_dir, decision.before_text_blob.as_deref())?,
        after_text: decision_blob(decision_dir, decision.after_text_blob.as_deref())?,
        source_device_id: decision.source_device_id.clone(),
        occurred_at_ms: decision.occurred_at_ms,
    })
}

/// 只允许 target-relative、无 `..`/根/前缀的普通路径；用于发布前校验 manifest
/// 里的文件身份，防止损坏决议把写入带出 target。
fn safe_target_rel_path(rel_path: &str) -> Option<&Path> {
    if rel_path.is_empty() {
        return None;
    }
    let path = Path::new(rel_path);
    if path.is_absolute() {
        return None;
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => {}
            _ => return None,
        }
    }
    Some(path)
}

enum FilePublish {
    Applied,
    Pending(String),
    Superseded(String),
}

fn publish_file_decision(
    live_root: &Path,
    decision_dir: &Path,
    decision: &SyncFileDecision,
) -> crate::error::Result<FilePublish> {
    let Some(rel_path) = safe_target_rel_path(&decision.rel_path) else {
        return Ok(FilePublish::Superseded(format!(
            "invalid sync commit target path: {}",
            decision.rel_path
        )));
    };
    let target = live_root.join(rel_path);
    let current = match std::fs::read(&target) {
        Ok(bytes) => Some(sha256_bytes(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(crate::error::Error::Io(std::io::Error::other(format!(
                "read sync commit target {}: {error}",
                target.display()
            ))))
        }
    };
    if decision.deleted {
        match current {
            None => return Ok(FilePublish::Applied),
            Some(current) if decision.before_sha256.as_deref() == Some(current.as_str()) => {}
            Some(_) => {
                return Ok(FilePublish::Superseded(format!(
                    "target changed before delete: {}",
                    decision.rel_path
                )))
            }
        }
        match std::fs::remove_file(&target) {
            Ok(()) => {
                crate::storage::sync_parent(&target)?;
                Ok(FilePublish::Applied)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(FilePublish::Applied)
            }
            Err(error) => Ok(FilePublish::Pending(format!(
                "delete {} failed: {error}",
                decision.rel_path
            ))),
        }
    } else {
        let Some(after_sha) = decision.after_sha256.as_deref() else {
            return Ok(FilePublish::Pending(format!(
                "apply decision has no after digest: {}",
                decision.rel_path
            )));
        };
        match current {
            Some(current) if current == after_sha => return Ok(FilePublish::Applied),
            Some(current) if decision.before_sha256.as_deref() == Some(current.as_str()) => {}
            None if decision.before_sha256.is_none() => {}
            _ => {
                return Ok(FilePublish::Superseded(format!(
                    "target changed before publish: {}",
                    decision.rel_path
                )))
            }
        }
        let Some(blob) = decision.blob.as_deref() else {
            return Ok(FilePublish::Pending(format!(
                "apply decision has no material: {}",
                decision.rel_path
            )));
        };
        let bytes = read_material(decision_dir, blob)?;
        if sha256_bytes(&bytes) != after_sha {
            return Ok(FilePublish::Pending(format!(
                "material digest mismatch for {}",
                decision.rel_path
            )));
        }
        match crate::storage::atomic_write_bytes(&target, &bytes) {
            Ok(()) => Ok(FilePublish::Applied),
            Err(error) => Ok(FilePublish::Pending(format!(
                "write {} failed: {error}",
                decision.rel_path
            ))),
        }
    }
}

struct PublishedDecision {
    applied_paths: Vec<CommittedPath>,
    chapter_receipts: Vec<ChapterSyncCommitReceipt>,
    chapter_changes: Vec<ChapterChangeReceipt>,
    pending: Vec<String>,
    superseded: Vec<String>,
    conflicts: Option<Vec<SyncConflict>>,
}

#[allow(
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_lines
)]
fn publish_manifest(
    dir: &Path,
    manifest: &SyncCommitManifest,
    guard: &crate::storage::WorkspaceWriteLock,
) -> crate::error::Result<PublishedDecision> {
    let live_root = Path::new(&manifest.target.live_root);
    log::debug!(
        "[sync] publishing commit decision {} (decided_at_ms={}, {} chapter(s), {} file(s))",
        manifest.operation_id,
        manifest.decided_at_ms,
        manifest.chapters.len(),
        manifest.files.len()
    );
    let mut applied_paths = Vec::new();
    let mut chapter_receipts = Vec::new();
    let mut chapter_changes = Vec::new();
    let mut pending = Vec::new();
    let mut superseded = Vec::new();
    let mut applied_file_paths = HashSet::new();
    let mut applied_chapters = HashSet::new();

    for decision in &manifest.chapters {
        match publish_chapter_decision(live_root, guard, dir, decision) {
            Ok(ChapterPublish::Applied) => {
                applied_chapters.insert(decision.key.clone());
                chapter_changes.push(chapter_change_receipt(decision, &manifest.operation_id));
                // 回执文本材料缺失是磁盘事实损坏：保留 manifest 待下次恢复，
                // 但绝不能因此隐藏已经发布的章节身份。
                match chapter_sync_receipt(decision, dir) {
                    Ok(receipt) => chapter_receipts.push(receipt),
                    Err(error) => pending.push(format!(
                        "chapter {}/{}/{} receipt: {error}",
                        decision.key.project_id, decision.key.volume_id, decision.key.chapter_id
                    )),
                }
                for (rel_path, deleted) in chapter_written_paths(live_root, decision) {
                    applied_paths.push(CommittedPath {
                        path: to_workspace_rel_path(
                            &manifest.target.kind,
                            manifest.target.project_id.as_deref(),
                            &rel_path,
                        ),
                        deleted,
                    });
                }
            }
            Ok(ChapterPublish::Superseded(reason)) => {
                superseded.push(format!(
                    "chapter {}/{}/{}: {reason}",
                    decision.key.project_id,
                    decision.key.volume_id,
                    decision.key.chapter_id
                ));
            }
            Err(error) => pending.push(format!(
                "chapter {}/{}/{}: {error}",
                decision.key.project_id, decision.key.volume_id, decision.key.chapter_id
            )),
        }
    }

    // 章节是同一份决定里的前置对象：只有全部章节都按决定发布，才允许把
    // state/索引/冲突材料（含三方基准）写上去；否则基准会对应到未应用的正文。
    let chapters_pending = !pending.is_empty();
    let chapters_superseded = !superseded.is_empty();
    if chapters_superseded {
        for decision in &manifest.files {
            superseded.push(format!(
                "file {}: not published because a chapter decision was superseded",
                decision.rel_path
            ));
        }
    } else if chapters_pending {
        for decision in &manifest.files {
            pending.push(format!(
                "file {}: deferred until chapter decisions publish",
                decision.rel_path
            ));
        }
    } else {
        // 文件决定按固定顺序发布（内容/索引在前，state/冲突在后）。一旦前面的
        // 文件被更新状态取代或暂时写不动，后面的 state/冲突基准就不能越过它
        // 单独落盘，否则基准会对应到未发布的业务对象。
        let mut file_barrier: Option<&'static str> = None;
        for decision in &manifest.files {
            if let Some(reason) = file_barrier {
                if reason == "pending" {
                    pending.push(format!(
                        "file {}: deferred after an earlier pending object",
                        decision.rel_path
                    ));
                } else {
                    superseded.push(format!(
                        "file {}: not published after an earlier superseded object",
                        decision.rel_path
                    ));
                }
                continue;
            }
            if let Some(key) = decision.chapter_dependency.as_ref() {
                if !applied_chapters.contains(key) {
                    superseded.push(format!(
                        "file {}: dependent chapter {}/{}/{} was not published",
                        decision.rel_path, key.project_id, key.volume_id, key.chapter_id
                    ));
                    file_barrier = Some("superseded");
                    continue;
                }
            }
            match publish_file_decision(live_root, dir, decision) {
                Ok(FilePublish::Applied) => {
                    applied_file_paths.insert(decision.rel_path.clone());
                    applied_paths.push(CommittedPath {
                        path: to_workspace_rel_path(
                            &manifest.target.kind,
                            manifest.target.project_id.as_deref(),
                            Path::new(&decision.rel_path),
                        ),
                        deleted: decision.deleted,
                    });
                }
                Ok(FilePublish::Pending(error)) => {
                    pending.push(error);
                    file_barrier = Some("pending");
                }
                Ok(FilePublish::Superseded(reason)) => {
                    superseded.push(reason);
                    file_barrier = Some("superseded");
                }
                Err(error) => {
                    pending.push(format!("file {}: {error}", decision.rel_path));
                    file_barrier = Some("pending");
                }
            }
        }
    }

    let conflict_records_rel = manifest
        .target
        .app_group
        .as_ref()
        .map(AppDataGroup::local_conflicts_path)
        .unwrap_or_else(|| "app-meta/sync/conflicts.json".to_string());
    let conflicts = if manifest.conflicts_changed
        && applied_file_paths.contains(&conflict_records_rel)
    {
        Some(manifest.conflicts.clone())
    } else {
        None
    };

    // 章节被更新的本地状态取代时这份决议已经不可能完整落地；此时必须收尾
    // （否则恢复入口会永远把文件当作待发布）。仍有 pending 但没有 superseded
    // 时保留 manifest，等待恢复入口继续发布。
    let completed = pending.is_empty() || !superseded.is_empty();
    if completed {
        finish_manifest(dir, manifest);
    }

    Ok(PublishedDecision {
        applied_paths,
        chapter_receipts,
        chapter_changes,
        pending,
        superseded,
        conflicts,
    })
}

// ---------------------------------------------------------------------------
// 提交入口
// ---------------------------------------------------------------------------

pub(crate) enum TargetCommitResult {
    Ok,
    Skipped,
    /// 决议已 durable，部分对象已发布，其余需要恢复入口继续发布。
    RecoveryPending(String),
    Failed(String),
}

/// 单 target 提交的完整结果。`committed_paths` 只包含真实已发布（或已确认
/// 达到决定状态）的对象；失败结果同样携带这些身份。
pub(crate) struct TargetCommitOutcome {
    pub result: TargetCommitResult,
    /// 本次发布后的 canonical 冲突列表；只有冲突记录真实落盘时才为 Some。
    pub conflicts: Option<Vec<SyncConflict>>,
    pub committed_paths: Vec<CommittedPath>,
    pub chapter_changes: Vec<ChapterChangeReceipt>,
    pub chapter_receipts: Vec<ChapterSyncCommitReceipt>,
    pub publish: PublishBatch,
}

impl TargetCommitOutcome {
    fn failure(message: String) -> Self {
        Self {
            result: TargetCommitResult::Failed(message),
            conflicts: None,
            committed_paths: Vec::new(),
            chapter_changes: Vec::new(),
            chapter_receipts: Vec::new(),
            publish: PublishBatch::default(),
        }
    }
}

struct CommitPublication {
    result: TargetCommitResult,
    applied_paths: Vec<CommittedPath>,
    chapter_receipts: Vec<ChapterSyncCommitReceipt>,
    chapter_changes: Vec<ChapterChangeReceipt>,
    conflicts: Option<Vec<SyncConflict>>,
}

/// 把统一裁决写成单一 target durable manifest，再逐条幂等发布。
///
/// 任何 live 写入都发生在 manifest durable 之后；规划/材料/manifest 失败时
/// live 未被改动，可以安全返回普通失败。
#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
fn commit_object_plan(
    run: &StagingRun,
    target: &TargetSyncResult,
    plan: &CommitPlan,
    content_actions: &[CommitAction],
    guard: &crate::storage::WorkspaceWriteLock,
    operation_id: &str,
) -> crate::error::Result<CommitPublication> {
    let live_root = run.target_live_root();
    let dir = decision_dir(live_root, run.app_group());
    reset_orphan_decision_dir(&dir);

    let default_project_id = live_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let chapter_sources = staging_chapter_sources(run, Some(&default_project_id));
    let (chapter_decisions, handled_chapters) = plan_chapter_decisions(
        run,
        guard,
        content_actions,
        &chapter_sources,
        &dir,
    )?;

    let mut filtered_actions: Vec<CommitAction> = content_actions
        .iter()
        .filter(|action| {
            let rel_path = match action {
                CommitAction::Apply { rel_path, .. } | CommitAction::Delete { rel_path } => {
                    rel_path
                }
            };
            match chapter_key_from_target_action(rel_path, &default_project_id) {
                Some(key) => !handled_chapters.contains(&key),
                None => true,
            }
        })
        .cloned()
        .collect();
    let anchor_actions = rebase_chapter_anchors(live_root, &filtered_actions)?;

    let decided_at_ms = chrono::Utc::now().timestamp_millis();
    let mut files = Vec::new();
    let mut file_index = HashMap::new();
    let conflict_decision = plan_conflict_decisions(
        run,
        plan,
        target,
        &dir,
        decided_at_ms,
        &mut files,
        &mut file_index,
    )?;
    for action in plan
        .engine_state_actions
        .iter()
        .chain(filtered_actions.iter())
    {
        push_action_file_decision(
            &dir,
            live_root,
            &mut files,
            &mut file_index,
            action,
        )?;
    }
    for (action, key) in anchor_actions {
        push_anchor_file_decision(
            &dir,
            live_root,
            &mut files,
            &mut file_index,
            &action,
            key,
        )?;
    }
    let canonical_conflicts = build_state_file_decisions(
        run,
        plan,
        &conflict_decision,
        &dir,
        &mut files,
        &mut file_index,
    )?;

    let manifest = SyncCommitManifest {
        schema_version: SYNC_COMMIT_SCHEMA_VERSION,
        phase: SyncCommitPhase::Decided,
        target: SyncCommitTargetIdentity {
            kind: target.target_kind.clone(),
            project_id: target.project_id.clone(),
            app_group: run.app_group().cloned(),
            live_root: live_root.to_string_lossy().into_owned(),
        },
        run_id: run.run_id().to_string(),
        operation_id: operation_id.to_string(),
        decided_at_ms,
        chapters: chapter_decisions,
        files,
        conflicts: canonical_conflicts,
        conflicts_changed: conflict_decision.changed,
    };
    // manifest 是唯一 Decided 点：材料先 durable，manifest 写成功后才发布。
    write_manifest(&dir, &manifest)?;

    let published = publish_manifest(&dir, &manifest, guard)?;
    let result = if !published.superseded.is_empty() {
        let pending_note = if published.pending.is_empty() {
            String::new()
        } else {
            format!(
                "; {} object(s) also remain pending",
                published.pending.len()
            )
        };
        TargetCommitResult::Failed(format!(
            "sync commit decision {} applied {} object(s); {} object(s) were superseded by newer local state: {}{}",
            run.run_id(),
            published.applied_paths.len(),
            published.superseded.len(),
            published.superseded.join(", "),
            pending_note
        ))
    } else if !published.pending.is_empty() {
        TargetCommitResult::RecoveryPending(format!(
            "sync commit decision {} applied {} object(s); {} object(s) still await recovery: {}",
            run.run_id(),
            published.applied_paths.len(),
            published.pending.len(),
            published.pending.join(", ")
        ))
    } else {
        TargetCommitResult::Ok
    };

    Ok(CommitPublication {
        result,
        applied_paths: published.applied_paths,
        chapter_receipts: published.chapter_receipts,
        chapter_changes: published.chapter_changes,
        conflicts: published.conflicts,
    })
}

fn recover_decision_for_target(
    live_root: &Path,
    target: &TargetSyncResult,
    group: Option<&AppDataGroup>,
    guard: &crate::storage::WorkspaceWriteLock,
) -> crate::error::Result<Option<PublishedDecision>> {
    let dir = decision_dir(live_root, group);
    let Some(manifest) = load_manifest(&dir)? else {
        return Ok(None);
    };
    if manifest.phase == SyncCommitPhase::Completed {
        remove_completed_decision(&dir, &manifest);
        return Ok(None);
    }
    if manifest.target.kind != target.target_kind
        || manifest.target.project_id != target.project_id
        || manifest.target.app_group.as_ref() != group
        || !same_directory(Path::new(&manifest.target.live_root), live_root)
    {
        return Err(crate::error::Error::Other(format!(
            "sync commit decision {} belongs to another target ({})",
            dir.display(),
            manifest.target.kind
        )));
    }
    publish_manifest(&dir, &manifest, guard).map(Some)
}

/// 单 target staging commit：先完成上一次未完成的 durable 决议，再规划并发布
/// 本次唯一决议。
///
/// 只有 `manifest` 完全发布（全部对象 applied）时，才把本轮真实成功的上传
/// 对象、远端删除与最终对象索引冻结成 [`PublishBatch`]；失败/恢复中的 target
/// 不产生任何发布事实。
#[allow(
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_lines
)]
pub(crate) fn commit_single_target_staging(
    run: &StagingRun,
    target: &TargetSyncResult,
    operation_id: &str,
) -> TargetCommitOutcome {
    let mode = target_commit_mode(&target.result.status);

    // 直接调用本入口的提交也必须遵守与 MCP 共享的工作区锁约定。
    let workspace_lock = match mode {
        TargetCommitMode::Skip => None,
        _ => match crate::storage::WorkspaceWriteLock::acquire(run.app_data_root()) {
            Ok(lock) => Some(lock),
            Err(error) => {
                return TargetCommitOutcome::failure(format!(
                    "workspace_write_lock_failed: {error}"
                ))
            }
        },
    };

    match mode {
        TargetCommitMode::Skip => {
            log::warn!(
                "Staging commit: skipping target (run_id={})",
                run.run_id()
            );
            TargetCommitOutcome {
                result: TargetCommitResult::Skipped,
                conflicts: None,
                committed_paths: Vec::new(),
                chapter_changes: Vec::new(),
                chapter_receipts: Vec::new(),
                publish: PublishBatch::default(),
            }
        }
        TargetCommitMode::Full | TargetCommitMode::ConflictMetadataOnly => {
            let Some(guard) = workspace_lock.as_ref() else {
                return TargetCommitOutcome::failure(
                    "workspace_write_lock_failed: lock guard missing".to_string(),
                );
            };
            let live_root = run.target_live_root();

            // 1. 上次中断留下的同一个 target 决议必须先按原决定发布完，不能
            //    在它上面再叠一份新决定。
            let recovered =
                match recover_decision_for_target(live_root, target, run.app_group(), guard) {
                Ok(recovered) => recovered,
                Err(error) => {
                    return TargetCommitOutcome::failure(format!(
                        "sync_commit_recovery_failed: {error}"
                    ))
                }
            };
            let mut committed_paths = Vec::new();
            let mut chapter_receipts = Vec::new();
            let mut chapter_changes = Vec::new();
            let mut recovered_conflicts = None;
            if let Some(recovered) = recovered {
                committed_paths.extend(recovered.applied_paths);
                chapter_receipts.extend(recovered.chapter_receipts);
                chapter_changes.extend(recovered.chapter_changes);
                recovered_conflicts = recovered.conflicts;
                if !recovered.pending.is_empty() {
                    log::warn!(
                        "[sync] target {} still has {} object(s) awaiting recovery: {}",
                        run.run_id(),
                        recovered.pending.len(),
                        recovered.pending.join(", ")
                    );
                    return TargetCommitOutcome {
                        result: TargetCommitResult::RecoveryPending(format!(
                            "previous sync commit decision for this target applied {} object(s); \
                             {} object(s) still await recovery: {}",
                            committed_paths.len(),
                            recovered.pending.len(),
                            recovered.pending.join(", ")
                        )),
                        conflicts: recovered_conflicts,
                        committed_paths,
                        chapter_changes,
                        chapter_receipts,
                        publish: PublishBatch::default(),
                    };
                }
                if !recovered.superseded.is_empty() {
                    log::warn!(
                        "[sync] previous sync commit decision had {} superseded object(s): {}",
                        recovered.superseded.len(),
                        recovered.superseded.join(", ")
                    );
                }
            }

            // 2. 账本在同一把工作区锁内读取一次，直接交给唯一裁决。
            let ledger = match crate::sync::local_mutations::latest_ledger(live_root) {
                Ok(ledger) => ledger,
                Err(error) => {
                    let msg = format!("read mutation ledger failed: {error}");
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return TargetCommitOutcome::failure(msg);
                }
            };
            let plan = match run.compute_commit_plan(live_root, &ledger) {
                Ok(plan) => plan,
                Err(error) => {
                    let msg = format!("compute_commit_plan failed: {}", error);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return TargetCommitOutcome::failure(msg);
                }
            };
            let actions = plan.content_actions.clone();
            let publication = match commit_object_plan(
                run,
                target,
                &plan,
                &actions,
                guard,
                operation_id,
            ) {
                Ok(publication) => publication,
                Err(error) => {
                    let msg = format!("commit_object_plan failed: {}", error);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return TargetCommitOutcome::failure(msg);
                }
            };

            committed_paths.extend(publication.applied_paths);
            chapter_receipts.extend(publication.chapter_receipts);
            chapter_changes.extend(publication.chapter_changes);
            let conflicts = publication.conflicts.or(recovered_conflicts);

            let publish = if matches!(&publication.result, TargetCommitResult::Ok) {
                let deleted = committed_paths.iter().filter(|path| path.deleted).count();
                log::info!(
                    "[sync] target commit decision {} published {} object(s) ({} delete(s))",
                    run.run_id(),
                    committed_paths.len(),
                    deleted
                );
                invalidate_correction_result_caches(
                    run.app_data_root(),
                    live_root,
                    target.project_id.as_deref(),
                    &actions,
                );
                PublishBatch {
                    objects: plan.uploads.clone(),
                    deletes: plan.publish_deletes.clone(),
                    delete_mutations: plan.delete_facts.clone(),
                    manifest: plan.remote_manifest.clone(),
                }
            } else {
                PublishBatch::default()
            };

            TargetCommitOutcome {
                result: publication.result,
                conflicts,
                committed_paths,
                chapter_changes,
                chapter_receipts,
                publish,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 启动恢复
// ---------------------------------------------------------------------------

/// 一次启动恢复完成的 target 事实。
#[derive(Debug, Default)]
pub(crate) struct RecoveredSyncCommit {
    pub target_live_root: PathBuf,
    pub applied_paths: Vec<CommittedPath>,
    pub chapter_changes: Vec<ChapterChangeReceipt>,
    pub pending: Vec<String>,
    pub superseded: Vec<String>,
}

/// 进程启动的 sync recovery：扫描所有固定决议路径，继续发布未完成决定。
///
/// 只按 manifest 里的 before/after 决定发布；已经达到决定状态的条目直接确认，
/// 绝不重新比较哪一边更新。重复调用幂等。
#[allow(clippy::too_many_lines)]
pub(crate) fn recover_pending_sync_commits(
    app_data_root: &Path,
    projects_root: &Path,
) -> Vec<RecoveredSyncCommit> {
    let mut results = Vec::new();
    let mut decision_dirs: Vec<(PathBuf, PathBuf)> = Vec::new();
    decision_dirs.push((
        app_data_root.join(SYNC_COMMIT_DECISION_DIR),
        app_data_root.to_path_buf(),
    ));
    if let Ok(entries) = std::fs::read_dir(app_data_root.join("app-meta/sync/groups")) {
        for entry in entries.flatten() {
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                decision_dirs.push((
                    entry.path().join("commit-decision"),
                    app_data_root.to_path_buf(),
                ));
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(projects_root) {
        for entry in entries.flatten() {
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                let project_root = entry.path();
                decision_dirs.push((
                    project_root.join(SYNC_COMMIT_DECISION_DIR),
                    project_root,
                ));
            }
        }
    }
    if decision_dirs
        .iter()
        .all(|(dir, _)| !manifest_path(dir).exists())
    {
        return results;
    }

    let guard = match crate::storage::WorkspaceWriteLock::acquire(app_data_root) {
        Ok(guard) => guard,
        Err(error) => {
            log::warn!(
                "[sync] commit recovery skipped: workspace write lock failed: {error}"
            );
            return results;
        }
    };
    for (dir, expected_live_root) in decision_dirs {
        let manifest = match load_manifest(&dir) {
            Ok(Some(manifest)) => manifest,
            Ok(None) => continue,
            Err(error) => {
                log::warn!(
                    "[sync] commit recovery: unreadable decision {}: {error}",
                    dir.display()
                );
                continue;
            }
        };
        if manifest.phase == SyncCommitPhase::Completed {
            remove_completed_decision(&dir, &manifest);
            continue;
        }
        let live_root = PathBuf::from(&manifest.target.live_root);
        if !same_directory(&live_root, &expected_live_root) {
            log::warn!(
                "[sync] commit recovery: decision {} records live root {} but is stored under {}; retained",
                dir.display(),
                live_root.display(),
                expected_live_root.display()
            );
            continue;
        }
        if !live_root.is_dir() {
            log::warn!(
                "[sync] commit recovery: target live root {} no longer exists; decision {} retained",
                live_root.display(),
                dir.display()
            );
            continue;
        }
        // 与正常 sync commit 相同锁序：workspace lock 在外，target state lock
        // 在内；启动恢复也必须和 UI 冲突解决串行，不能绕过 state lock。
        let published_result = crate::sync::state_lock::with_sync_state_lock(&live_root, || {
            publish_manifest(&dir, &manifest, &guard)
        });
        match published_result {
            Ok(published) => {
                if !published.pending.is_empty() {
                    log::warn!(
                        "[sync] commit recovery: {} object(s) still pending for {}: {}",
                        published.pending.len(),
                        live_root.display(),
                        published.pending.join(", ")
                    );
                }
                for superseded in &published.superseded {
                    log::warn!(
                        "[sync] commit recovery: superseded object for {}: {}",
                        live_root.display(),
                        superseded
                    );
                }
                results.push(RecoveredSyncCommit {
                    target_live_root: live_root,
                    applied_paths: published.applied_paths,
                    chapter_changes: published.chapter_changes,
                    pending: published.pending,
                    superseded: published.superseded,
                });
            }
            Err(error) => log::warn!(
                "[sync] commit recovery failed for {}: {error} (decision retained)",
                dir.display()
            ),
        }
    }
    results
}

// ---------------------------------------------------------------------------
// 失败注入
// ---------------------------------------------------------------------------

/// 把 staging commit 失败注入 target 结果。
///
/// 清单读取/解析失败带 `manifest_read_failed: path=... cause=...`；保留具体
/// 路径并归到 merge 阶段，避免把损坏的清单误报成传输失败。
pub(crate) fn apply_commit_failure_to_target(
    target: &mut crate::sync::types::TargetSyncResult,
    message: &str,
) {
    target.result.status =
        crate::sync::SyncStatus::RecoverableError(format!("staging_commit_failed: {message}"));
    target.result.error = Some(format!("staging commit failed: {message}"));

    if message.contains("manifest_read_failed:") {
        target.failure_phase = Some("merge".to_string());
        if let Some((_, path)) = message.split_once("path=") {
            let path = path
                .split(|character: char| character.is_whitespace() || character == ',')
                .next()
                .unwrap_or_default();
            if !path.is_empty() {
                target.failure_path = Some(path.to_string());
            }
        }
    }
}

/// 决议已 durable、部分对象已发布，但仍有条目等待恢复。
pub(crate) fn apply_commit_recovery_pending_to_target(
    target: &mut crate::sync::types::TargetSyncResult,
    message: &str,
) {
    target.result.status = crate::sync::SyncStatus::RecoverableError(format!(
        "staging_commit_recovery_pending: {message}"
    ));
    target.result.error = Some(format!(
        "staging commit applied partially; durable recovery is pending: {message}"
    ));
}

pub(crate) enum TargetCommitMode {
    Full,
    ConflictMetadataOnly,
    Skip,
}

pub(crate) fn target_commit_mode(status: &crate::sync::SyncStatus) -> TargetCommitMode {
    use crate::sync::SyncStatus;
    match status {
        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied => {
            TargetCommitMode::Full
        }
        SyncStatus::Conflict | SyncStatus::PartialConflict => {
            TargetCommitMode::ConflictMetadataOnly
        }
        _ => TargetCommitMode::Skip,
    }
}
