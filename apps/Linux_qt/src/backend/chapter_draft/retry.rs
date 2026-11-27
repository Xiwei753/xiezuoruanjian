// =============================================================================
// chapter_draft/retry.rs — 可自动重试草稿的独立槽位
// =============================================================================
//
// Issue #1062 评论 7195690539 第 1 项：恢复失败但“下次进入还能再试”的事务
// （Core ack 对账、待转换输入、读章节失败、事件链暂时不完整等）不能继续占用
// `chapter-drafts/<chapter_id>/` 这个可写槽位——否则编辑器新会话的 `note_text`
// 会把新会话的正文/基线写进旧事务，事实与正文永久混合。
//
// 做法：失败时把整目录 rename 到 `chapter-draft-kept/<chapter>/<stamp>/`
// （不可写、永不被扫描成待恢复草稿），再在
// `app-meta/chapter-draft-retry/<chapter_id>/<stamp>.json` 留一条重试标记。
// 下一次进入作品时，恢复 actor 先按标记把归档目录原子 rename 回活动槽位再
// 重试；重试期间编辑器尚未打开，不会与新会话并发。若活动槽位已有新会话草稿，
// 绝不覆盖：旧副本继续留在归档区。
//
// Issue #1062 评论 #7210163429 第 1/2 项：重试账本按**归档事务**记账，同一章
// 可以同时挂起多份旧草稿（A、B…），每条标记绑定自己的 `archived_path`；
// 登记/删除/搬回都只作用于该事务，绝不按 chapter_id 连带清理别的事务的标记。
// 同章活动槽位一次只装入一份，其余归档等下一次进入再处理。
//
// Issue #1062 评论 #7210360212 第 2 项：重试意图先写进草稿目录自己的
// `retry-intent.json`，再与草稿原件一起 rename 进归档区；外部
// `chapter-draft-retry/<chapter>/<stamp>.json` 只是可用意图重建的索引。
// 这样“归档完成、索引未写”之间崩溃也不会把待恢复事务变成仅人工归档。

use std::path::{Path, PathBuf};

use super::store::{self, DraftLocation};

pub(super) const RETRY_DIR: &str = "chapter-draft-retry";
/// 写在草稿目录内部、随目录一起 rename 的重试意图。
pub(super) const RETRY_INTENT_FILE: &str = "retry-intent.json";
const RETRY_SCHEMA_VERSION: u32 = 1;

/// 一条可自动重试的事务标记。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DraftRetryMarker {
    pub schema_version: u32,
    pub chapter_id: String,
    pub volume_id: String,
    pub archived_path: String,
    pub stamp: String,
    pub reason: String,
    pub attempts: u32,
    pub updated_at_ms: i64,
}

impl Default for DraftRetryMarker {
    fn default() -> Self {
        Self {
            schema_version: RETRY_SCHEMA_VERSION,
            chapter_id: String::new(),
            volume_id: String::new(),
            archived_path: String::new(),
            stamp: String::new(),
            reason: String::new(),
            attempts: 0,
            updated_at_ms: 0,
        }
    }
}

/// 一份归档副本的重试标记（同章可以有多份，各自独立恢复）。
#[derive(Debug, Clone)]
pub(crate) struct RetryCandidate {
    pub marker: DraftRetryMarker,
    /// 指向这条候选的 durable 指针：索引 JSON 或归档目录里的意图文件。
    pub marker_path: PathBuf,
    /// true 表示候选来自归档目录里的意图（索引缺失后重建）。
    pub intent_backed: bool,
}

/// 归档目录内部的 retry-intent.json。
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
struct RetryIntent {
    schema_version: u32,
    chapter_id: String,
    volume_id: String,
    reason: String,
    attempts: u32,
    updated_at_ms: i64,
}

impl Default for RetryIntent {
    fn default() -> Self {
        Self {
            schema_version: RETRY_SCHEMA_VERSION,
            chapter_id: String::new(),
            volume_id: String::new(),
            reason: String::new(),
            attempts: 0,
            updated_at_ms: 0,
        }
    }
}

pub(crate) fn retry_root(projects_root: &Path, project_id: &str) -> PathBuf {
    projects_root
        .join(project_id)
        .join("app-meta")
        .join(RETRY_DIR)
}

pub(crate) fn retry_chapter_dir(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> PathBuf {
    retry_root(projects_root, project_id).join(chapter_id)
}

/// 把重试意图写进活动草稿目录，随后 `quarantine` 会把它和草稿一起 rename。
///
/// Issue #1062 评论 #7210360212 第 2 项：必须在 rename 之前 durable 写出；
/// 外部索引只是可重建的缓存。
pub(crate) fn write_retry_intent(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    reason: &str,
    previous_attempts: u32,
) -> Result<(), String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Err(format!(
            "write retry intent: invalid identity project={project_id} chapter={chapter_id}"
        ));
    };
    if !location.dir.is_dir() {
        return Err(format!(
            "write retry intent: draft dir missing for project={project_id} chapter={chapter_id}"
        ));
    }
    let intent = RetryIntent {
        schema_version: RETRY_SCHEMA_VERSION,
        chapter_id: chapter_id.to_string(),
        volume_id: volume_id.to_string(),
        reason: reason.to_string(),
        attempts: previous_attempts.saturating_add(1),
        updated_at_ms: super::now_ms(),
    };
    let json = serde_json::to_string_pretty(&intent)
        .map_err(|error| format!("encode retry intent: {error}"))?;
    writer_core::storage::atomic_write_string(&location.dir.join(RETRY_INTENT_FILE), &json)
        .map_err(|error| format!("write retry intent: {error}"))
}

/// 删除活动草稿目录里的重试意图（已改为人工归档或已成功恢复时）。
pub(crate) fn clear_retry_intent(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<(), String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(());
    };
    remove_marker(&location.dir.join(RETRY_INTENT_FILE))
}

fn read_intent(dir: &Path) -> Result<RetryIntent, String> {
    let path = dir.join(RETRY_INTENT_FILE);
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("read retry intent {}: {error}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("retry intent corrupted (kept): {}: {error}", path.display()))
}

/// 从归档目录内部意图重建候选（外部索引缺失时的唯一恢复入口）。
fn intent_candidates(projects_root: &Path, project_id: &str) -> Vec<RetryCandidate> {
    let kept = super::store::kept_root(projects_root, project_id);
    let Ok(chapters) = std::fs::read_dir(&kept) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for chapter in chapters.flatten() {
        let chapter_id = chapter
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        let Some(chapter_id) = chapter_id else {
            continue;
        };
        if !store::is_safe_id(&chapter_id) {
            continue;
        }
        let Ok(stamps) = std::fs::read_dir(chapter.path()) else {
            continue;
        };
        for stamp in stamps.flatten() {
            let path = stamp.path();
            if !path.is_dir() || !path.join(RETRY_INTENT_FILE).is_file() {
                continue;
            }
            let intent = match read_intent(&path) {
                Ok(intent) => intent,
                Err(error) => {
                    log::warn!("{error}");
                    continue;
                }
            };
            let stamp_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            candidates.push(RetryCandidate {
                marker: DraftRetryMarker {
                    schema_version: RETRY_SCHEMA_VERSION,
                    chapter_id: chapter_id.clone(),
                    volume_id: intent.volume_id,
                    archived_path: path.to_string_lossy().into_owned(),
                    stamp: stamp_name,
                    reason: intent.reason,
                    attempts: intent.attempts,
                    updated_at_ms: intent.updated_at_ms,
                },
                marker_path: path.join(RETRY_INTENT_FILE),
                intent_backed: true,
            });
        }
    }
    candidates
}

/// 登记一条可自动重试的事务：归档副本路径 + 原始失败原因。
///
/// 标记文件名由归档事务的目录名（`stamp`）决定：一次归档一条标记，重复登记
/// 同一副本只更新自己那一条。Issue #1062 评论 #7210163429 第 2 项：这里不再
/// 删除同章其它 JSON，否则更新的事务会把旧事务的恢复指针一起抹掉。
pub(crate) fn record_retry(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    archived: &Path,
    reason: &str,
    previous_attempts: u32,
) -> Result<DraftRetryMarker, String> {
    if !store::is_safe_id(project_id) || !store::is_safe_id(chapter_id) {
        return Err(format!(
            "record retry: invalid identity project={project_id} chapter={chapter_id}"
        ));
    }
    let dir = retry_chapter_dir(projects_root, project_id, chapter_id);
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("create retry dir {}: {error}", dir.display()))?;
    let stamp = archived
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("archived")
        .to_string();
    let marker = DraftRetryMarker {
        schema_version: RETRY_SCHEMA_VERSION,
        chapter_id: chapter_id.to_string(),
        volume_id: volume_id.to_string(),
        archived_path: archived.to_string_lossy().into_owned(),
        stamp: stamp.clone(),
        reason: reason.to_string(),
        attempts: previous_attempts.saturating_add(1),
        updated_at_ms: super::now_ms(),
    };
    let path = dir.join(format!("{stamp}.json"));
    let json = serde_json::to_string_pretty(&marker)
        .map_err(|error| format!("encode retry marker: {error}"))?;
    writer_core::storage::atomic_write_string(&path, &json)
        .map_err(|error| format!("write retry marker {}: {error}", path.display()))?;
    Ok(marker)
}

/// 项目下所有可自动重试的事务（每份归档副本一条，按章与生成时间排序）。
pub(crate) fn list_retries(projects_root: &Path, project_id: &str) -> Vec<RetryCandidate> {
    if !store::is_safe_id(project_id) {
        return Vec::new();
    }
    let root = retry_root(projects_root, project_id);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        candidates.extend(markers_in(&path));
    }
    // 索引缺失（rename 后、写索引前崩溃）时，从归档目录内部意图重建队列。
    candidates.extend(intent_candidates(projects_root, project_id));
    // 同一归档副本可能因崩溃留下同名的重复记录：按 archived_path 只保留最新
    // 一条，避免同一次重试被搬两遍。
    candidates.sort_by(|left, right| {
        (
            &left.marker.chapter_id,
            &left.marker.archived_path,
            left.marker.updated_at_ms,
            &left.marker.stamp,
        )
            .cmp(&(
                &right.marker.chapter_id,
                &right.marker.archived_path,
                right.marker.updated_at_ms,
                &right.marker.stamp,
            ))
    });
    let mut deduped: Vec<RetryCandidate> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match deduped.last_mut() {
            Some(last)
                if last.marker.chapter_id == candidate.marker.chapter_id
                    && last.marker.archived_path == candidate.marker.archived_path =>
            {
                // 同时间戳优先显式索引（含最新 attempts/reason）。
                let newer = candidate.marker.updated_at_ms > last.marker.updated_at_ms
                    || (candidate.marker.updated_at_ms == last.marker.updated_at_ms
                        && !candidate.intent_backed);
                if newer {
                    *last = candidate;
                }
            }
            _ => deduped.push(candidate),
        }
    }
    deduped.sort_by(|left, right| {
        (
            &left.marker.chapter_id,
            left.marker.updated_at_ms,
            &left.marker.stamp,
        )
            .cmp(&(
                &right.marker.chapter_id,
                right.marker.updated_at_ms,
                &right.marker.stamp,
            ))
    });
    deduped
}

fn read_marker(path: &Path) -> Result<DraftRetryMarker, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("read retry marker {}: {error}", path.display()))?;
    let marker: DraftRetryMarker = serde_json::from_str(&content)
        .map_err(|error| format!("retry marker corrupted (kept): {}: {error}", path.display()))?;
    if marker.chapter_id.is_empty() || marker.archived_path.is_empty() {
        return Err(format!(
            "retry marker incomplete (kept): {}",
            path.display()
        ));
    }
    Ok(marker)
}

fn markers_in(dir: &Path) -> Vec<RetryCandidate> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        match read_marker(&path) {
            Ok(marker) => candidates.push(RetryCandidate {
                marker,
                marker_path: path,
                intent_backed: false,
            }),
            Err(error) => log::warn!("{error}"),
        }
    }
    candidates
}

/// 取某章最早登记的重试事务（同章多份时按事务逐次 materialize）。
fn oldest_marker_in(dir: &Path) -> Option<RetryCandidate> {
    markers_in(dir).into_iter().min_by(|left, right| {
        (left.marker.updated_at_ms, &left.marker.stamp)
            .cmp(&(right.marker.updated_at_ms, &right.marker.stamp))
    })
}

/// 把可重试事务的归档目录搬回活动槽位，供恢复 actor 再次尝试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryMaterialize {
    /// 没有重试标记。
    NoMarker,
    /// 已搬回活动槽位，本轮可以正常重试。
    Moved,
    /// 活动槽位已有新会话草稿：绝不覆盖，保留归档，交给本轮正常恢复分支
    /// 处理活动草稿后再处理这条事务。
    ActiveDraftExists,
    /// 活动槽位只剩真实 pending-ack 文件（meta/正文/事实都已结算）：ack 还
    /// 承担事务坐标，必须先由 Core journal 对账消费，不能当空壳回收。
    AckOnlyPending,
    /// 归档目录已不存在（被人工清理）：该事务的标记已删除。
    ArchivedMissing,
}

pub(crate) fn materialize_retry(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<RetryMaterialize, String> {
    if DraftLocation::new(projects_root, project_id, chapter_id).is_none() {
        return Ok(RetryMaterialize::NoMarker);
    }
    let Some(candidate) =
        oldest_marker_in(&retry_chapter_dir(projects_root, project_id, chapter_id))
    else {
        return Ok(RetryMaterialize::NoMarker);
    };
    materialize_retry_candidate(projects_root, project_id, chapter_id, &candidate)
}

/// 按具体归档事务（标记文件）搬回活动槽位。
pub(crate) fn materialize_retry_candidate(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
    candidate: &RetryCandidate,
) -> Result<RetryMaterialize, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(RetryMaterialize::NoMarker);
    };
    let archived = PathBuf::from(&candidate.marker.archived_path);
    if !archived.is_dir() {
        remove_marker(&candidate.marker_path)?;
        remove_retry_chapter_dir_if_empty(projects_root, project_id, chapter_id);
        return Ok(RetryMaterialize::ArchivedMissing);
    }
    if location.dir.exists() {
        if !store::is_terminal(&location) {
            if store::draft_dir_has_transaction_content(&location.dir) {
                return Ok(RetryMaterialize::ActiveDraftExists);
            }
            if store::draft_dir_has_ack_files(&location.dir) {
                // Issue #1062 评论 #7210163429 第 3 项：真实 pending-ack 不是
                // 空壳文件，先让对账把它消费到 Core journal，再决定槽位。
                return Ok(RetryMaterialize::AckOnlyPending);
            }
        }
        // 已 terminal 或确实为空（可能残留空目录）：先回收，重试副本才能搬回。
        store::remove_settled_storage(&location).map_err(|error| {
            format!(
                "remove ghost draft slot {}: {error}",
                location.dir.display()
            )
        })?;
    }
    if let Some(parent) = location.dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create draft dir {}: {error}", parent.display()))?;
    }
    std::fs::rename(&archived, &location.dir).map_err(|error| {
        format!(
            "materialize retry draft {} -> {}: {error}",
            archived.display(),
            location.dir.display()
        )
    })?;
    // 搬回成功：先清掉随目录搬进活动槽位的重试意图，再只删这一条事务的
    // 外部索引；同章其它归档副本的标记原样保留。
    remove_marker(&location.dir.join(RETRY_INTENT_FILE))?;
    if !candidate.intent_backed {
        remove_marker(&candidate.marker_path)?;
    }
    remove_retry_chapter_dir_if_empty(projects_root, project_id, chapter_id);
    Ok(RetryMaterialize::Moved)
}

fn remove_retry_chapter_dir_if_empty(projects_root: &Path, project_id: &str, chapter_id: &str) {
    let chapter_dir = retry_chapter_dir(projects_root, project_id, chapter_id);
    match std::fs::remove_dir(&chapter_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "remove retry chapter dir {}: {error}",
            chapter_dir.display()
        ),
    }
}

/// 精确删除一条事务标记（搬运成功、归档丢失或事务已终结时调用）。
pub(crate) fn remove_marker(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove retry marker {}: {error}", path.display())),
    }
}

/// 项目下待自动重试的章节 id（QML 状态与诊断用；同章多份事务只列一次）。
pub(crate) fn retry_chapters(projects_root: &Path, project_id: &str) -> Vec<String> {
    let mut chapters: Vec<String> = list_retries(projects_root, project_id)
        .into_iter()
        .map(|candidate| candidate.marker.chapter_id)
        .collect();
    chapters.sort();
    chapters.dedup();
    chapters
}

/// 有标记文件但无法解析的章节：历史归档保留，但本轮不能冒充活动草稿。
///
/// Issue #1062 评论 #7209683560 第 3 项要求扫描时显式报告无效标记。
pub(crate) fn unreadable_retry_chapters(projects_root: &Path, project_id: &str) -> Vec<String> {
    if !store::is_safe_id(project_id) {
        return Vec::new();
    }
    let root = retry_root(projects_root, project_id);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut chapters = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(chapter_id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !store::is_safe_id(chapter_id) {
            continue;
        }
        let Ok(marker_files) = std::fs::read_dir(&path) else {
            continue;
        };
        let mut unreadable = false;
        for marker_entry in marker_files.flatten() {
            let marker_path = marker_entry.path();
            if marker_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Err(error) = read_marker(&marker_path) {
                log::warn!("{error}");
                unreadable = true;
            }
        }
        if unreadable {
            chapters.push(chapter_id.to_string());
        }
    }
    chapters.sort();
    chapters.dedup();
    chapters
}
