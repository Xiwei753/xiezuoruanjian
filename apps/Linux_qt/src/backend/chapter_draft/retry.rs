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
// 绝不覆盖：旧副本继续留在归档区，过期标记在本轮结束时收敛。

use std::path::{Path, PathBuf};

use super::store::{self, DraftLocation};

pub(super) const RETRY_DIR: &str = "chapter-draft-retry";
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

/// 列出并加载某章的当前重试标记。
#[derive(Debug, Clone)]
pub(crate) struct RetryCandidate {
    pub marker: DraftRetryMarker,
    pub marker_path: PathBuf,
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

/// 登记一条可自动重试的事务：归档副本路径 + 原始失败原因。
///
/// 同一章只保留最新一条标记（旧标记先删除），避免多次失败堆积出互相矛盾的
/// 重试目标。
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
    // Issue #1062 评论 #7209683560 第 3 项：先把新标记 durable 写出，再删除
    // 旧标记。写失败/崩溃时旧标记仍然有效，历史归档不会失去自动恢复引用。
    let mut path = dir.join(format!("{stamp}.json"));
    let mut attempt = 1u32;
    while path.exists() {
        if attempt > 999 {
            return Err(format!(
                "record retry: no free marker name project={project_id} chapter={chapter_id} stamp={stamp}"
            ));
        }
        path = dir.join(format!("{stamp}-r{attempt}.json"));
        attempt = attempt.saturating_add(1);
    }
    let json = serde_json::to_string_pretty(&marker)
        .map_err(|error| format!("encode retry marker: {error}"))?;
    writer_core::storage::atomic_write_string(&path, &json)
        .map_err(|error| format!("write retry marker {}: {error}", path.display()))?;
    // 新标记已 durable，现在才回收同章的其它标记文件。
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let stale = entry.path();
            if stale == path || stale.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            if let Err(error) = std::fs::remove_file(&stale) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("remove old retry marker {}: {error}", stale.display());
                }
            }
        }
    }
    Ok(marker)
}

/// 项目下所有可自动重试的章节（每章最新一条标记）。
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
        let Some(latest) = latest_marker_in(&path) else {
            continue;
        };
        candidates.push(latest);
    }
    candidates.sort_by(|left, right| left.marker.chapter_id.cmp(&right.marker.chapter_id));
    candidates
}

fn latest_marker_in(dir: &Path) -> Option<RetryCandidate> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<RetryCandidate> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                log::warn!("read retry marker {}: {error}", path.display());
                continue;
            }
        };
        let marker: DraftRetryMarker = match serde_json::from_str(&content) {
            Ok(marker) => marker,
            Err(error) => {
                log::warn!("retry marker corrupted (kept): {}: {error}", path.display());
                continue;
            }
        };
        if marker.chapter_id.is_empty() || marker.archived_path.is_empty() {
            log::warn!("retry marker incomplete (kept): {}", path.display());
            continue;
        }
        let replace = best.as_ref().is_none_or(|current| {
            (marker.updated_at_ms, &marker.stamp)
                > (current.marker.updated_at_ms, &current.marker.stamp)
        });
        if replace {
            best = Some(RetryCandidate {
                marker,
                marker_path: path,
            });
        }
    }
    best
}

/// 把可重试事务的归档目录搬回活动槽位，供恢复 actor 再次尝试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryMaterialize {
    /// 没有重试标记。
    NoMarker,
    /// 已搬回活动槽位，本轮可以正常重试。
    Moved,
    /// 活动槽位已有新会话草稿：绝不覆盖，保留归档，交给本轮正常恢复分支
    /// 处理活动草稿后再收敛过期标记。
    ActiveDraftExists,
    /// 归档目录已不存在（被人工清理）：标记已删除，按已处理收敛。
    ArchivedMissing,
}

pub(crate) fn materialize_retry(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<RetryMaterialize, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(RetryMaterialize::NoMarker);
    };
    let Some(candidate) =
        latest_marker_in(&retry_chapter_dir(projects_root, project_id, chapter_id))
    else {
        return Ok(RetryMaterialize::NoMarker);
    };
    let archived = PathBuf::from(&candidate.marker.archived_path);
    if !archived.is_dir() {
        clear_retry(projects_root, project_id, chapter_id)?;
        return Ok(RetryMaterialize::ArchivedMissing);
    }
    if location.dir.exists() {
        if !store::is_terminal(&location) && store::draft_dir_has_content(&location.dir) {
            return Ok(RetryMaterialize::ActiveDraftExists);
        }
        // Issue #1062 评论 #7209683560 第 1 项：只有空目录/残留 ack 缓存/
        // 已 terminal 待回收的“幽灵槽位”不是活动草稿；先回收它，重试副本
        // 才能搬回。
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
    // 只删除被搬走的那条标记；同章旧标记在 record_retry 时已清理，这里再
    // 兜底收敛一次空目录。
    if let Err(error) = std::fs::remove_file(&candidate.marker_path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "remove materialized retry marker {}: {error}",
                candidate.marker_path.display()
            );
        }
    }
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
    Ok(RetryMaterialize::Moved)
}

/// 删除某章的重试标记（恢复成功、已永久归档、或归档丢失时调用）。
pub(crate) fn clear_retry(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<(), String> {
    if !store::is_safe_id(project_id) || !store::is_safe_id(chapter_id) {
        return Ok(());
    }
    let dir = retry_chapter_dir(projects_root, project_id, chapter_id);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove retry markers {}: {error}", dir.display())),
    }
}

/// 项目下待自动重试的章节 id（QML 状态与诊断用）。
pub(crate) fn retry_chapters(projects_root: &Path, project_id: &str) -> Vec<String> {
    list_retries(projects_root, project_id)
        .into_iter()
        .map(|candidate| candidate.marker.chapter_id)
        .collect()
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
        let has_marker_files = std::fs::read_dir(&path).is_ok_and(|mut entries| {
            entries.any(|entry| {
                entry.is_ok_and(|entry| {
                    entry.path().extension().and_then(|ext| ext.to_str()) == Some("json")
                })
            })
        });
        if has_marker_files && latest_marker_in(&path).is_none() {
            chapters.push(chapter_id.to_string());
        }
    }
    chapters.sort();
    chapters.dedup();
    chapters
}
