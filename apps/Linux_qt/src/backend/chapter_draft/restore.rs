// 草稿恢复计划与只读快照（UI/恢复命令使用）。

use std::collections::HashSet;
use std::path::Path;

use super::store::{self, DraftLocation};
use super::{
    lock_registry, ChapterDraft, ChapterDraftRestorePlan, ChapterDraftSnapshot, DraftFactIndex,
    DraftState, LoadedFact,
};

pub(crate) fn snapshot(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<ChapterDraftSnapshot> {
    let location = DraftLocation::new(projects_root, project_id, chapter_id)?;
    if let Some(state) = lock_registry().get(&location.key) {
        return Some(ChapterDraftSnapshot::from_state(state));
    }
    match store::read_new_state(&location, false) {
        Ok(Some(state)) => return Some(ChapterDraftSnapshot::from_state(&state)),
        Ok(None) => {}
        Err(error) => {
            log::warn!(
                "chapter draft unreadable (kept): project={project_id} chapter={chapter_id}: {error}"
            );
            return None;
        }
    }
    match store::read_legacy_checked(&location.legacy_path(), project_id, chapter_id) {
        Ok(Some(legacy)) => Some(ChapterDraftSnapshot::from_state(
            &store::legacy_state(&location, legacy).0,
        )),
        Ok(None) => None,
        Err(error) => {
            log::warn!(
                "legacy chapter draft unreadable (kept): project={project_id} chapter={chapter_id}: {error}"
            );
            None
        }
    }
}

/// 一份“不可读/未决”的恢复候选：保留路径与错误，章节进入未决状态。
#[derive(Debug, Clone)]
pub(crate) struct UnreadableDraft {
    pub chapter_id: String,
    pub path: String,
    pub error: String,
}

/// 项目恢复发现结果：草稿候选 + 无法读取的原件 + 扫描错误。
///
/// Issue #1062 评论 #7213634487 第 A 项：UI 等待数与恢复 actor 必须使用同一份
/// 严格清单；损坏的旧格式文件不能被当成“没有草稿”。
#[derive(Debug, Clone, Default)]
pub(crate) struct DraftDiscovery {
    /// 可恢复候选（含读不出来的草稿摘要，用于让该章保持未决）。
    pub drafts: Vec<ChapterDraft>,
    /// 有原件但读不出来：保留路径与错误，章节不得被判成已完成。
    pub unreadable: Vec<UnreadableDraft>,
    /// 目录迭代错误等扫描级问题（仍返回已发现部分，由调用方决定阻断）。
    pub scan_errors: Vec<String>,
}

/// 严格发现项目内需要恢复的草稿。
pub(crate) fn discover_project_drafts(
    projects_root: &Path,
    project_id: &str,
) -> Result<DraftDiscovery, String> {
    let mut discovery = DraftDiscovery::default();
    if !store::is_safe_id(project_id) {
        return Ok(discovery);
    }
    let workspace = store::workspace_key_of(projects_root);
    let root = store::drafts_root(projects_root, project_id);
    {
        let registry = lock_registry();
        discovery.drafts = registry
            .values()
            .filter(|state| state.project_id == project_id && state.workspace_key == workspace)
            .map(DraftState::summary)
            .collect();
    }
    discovery
        .drafts
        .sort_by(|left, right| left.chapter_id.cmp(&right.chapter_id));
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(discovery);
        }
        Err(error) => {
            return Err(format!(
                "read chapter drafts dir {}: {error}",
                root.display()
            ))
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                discovery.scan_errors.push(format!(
                    "read chapter drafts entry {}: {error}",
                    root.display()
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            let Some(chapter_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !store::is_safe_id(chapter_id)
                || discovery
                    .drafts
                    .iter()
                    .any(|draft| draft.chapter_id == chapter_id)
            {
                continue;
            }
            match store::read_summary_from_dir(projects_root, &path, project_id, chapter_id) {
                Ok(Some(summary)) => discovery.drafts.push(summary),
                Ok(None) => {
                    // Issue #1062 评论 7195690539 第 3 项：已写 terminal 决议、
                    // 只差目录回收的目录不是“损坏草稿”，不能又被当成恢复候选
                    // 搬进归档区；这里补做一次 GC 后跳过。
                    if let Some(location) =
                        store::DraftLocation::new(projects_root, project_id, chapter_id)
                    {
                        if store::is_terminal(&location) {
                            if let Err(error) = store::remove_settled_storage(&location) {
                                log::warn!(
                                    "settled chapter draft gc deferred: project={project_id} chapter={chapter_id}: {error}"
                                );
                            }
                            continue;
                        }
                    }
                    if store::draft_dir_has_content(&path) {
                        discovery.drafts.push(store::unreadable_summary(
                            &path,
                            project_id,
                            chapter_id,
                            String::new(),
                        ));
                        discovery.unreadable.push(UnreadableDraft {
                            chapter_id: chapter_id.to_string(),
                            path: path.display().to_string(),
                            error: "chapter draft meta missing (kept)".to_string(),
                        });
                    }
                }
                Err(error) => {
                    discovery.drafts.push(store::unreadable_summary(
                        &path,
                        project_id,
                        chapter_id,
                        error.clone(),
                    ));
                    discovery.unreadable.push(UnreadableDraft {
                        chapter_id: chapter_id.to_string(),
                        path: path.display().to_string(),
                        error,
                    });
                }
            }
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(chapter_id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if !store::is_safe_id(chapter_id)
            || discovery
                .drafts
                .iter()
                .any(|draft| draft.chapter_id == chapter_id)
        {
            continue;
        }
        match store::read_legacy_checked(&path, project_id, chapter_id) {
            Ok(Some(legacy)) => {
                let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id)
                else {
                    continue;
                };
                discovery
                    .drafts
                    .push(store::legacy_state(&location, legacy).0.summary());
            }
            Ok(None) => {}
            Err(error) => {
                // 损坏/身份不符的旧格式原件必须保持未决：产生候选并保留路径。
                discovery.drafts.push(store::unreadable_summary(
                    &path,
                    project_id,
                    chapter_id,
                    error.clone(),
                ));
                discovery.unreadable.push(UnreadableDraft {
                    chapter_id: chapter_id.to_string(),
                    path: path.display().to_string(),
                    error,
                });
            }
        }
    }
    discovery
        .drafts
        .sort_by(|left, right| left.chapter_id.cmp(&right.chapter_id));
    Ok(discovery)
}

/// 兼容 UI 的宽松入口：扫描失败只记日志，语义由 `discover_project_drafts` 决定。
pub(crate) fn list_project_drafts(projects_root: &Path, project_id: &str) -> Vec<ChapterDraft> {
    match discover_project_drafts(projects_root, project_id) {
        Ok(discovery) => {
            for error in discovery.scan_errors {
                log::warn!("chapter draft discovery: {error}");
            }
            discovery.drafts
        }
        Err(error) => {
            log::warn!("chapter draft discovery failed: {error}");
            Vec::new()
        }
    }
}

/// 读取草稿的恢复计划；任何文件缺失/损坏/正文摘要不一致都返回 Err，
/// 且不删除任何草稿文件（保留给下一次重试或人工排查）。
///
/// `Ok(None)` 只表示“没有草稿/meta”，与“存在但读不出来”严格区分。
pub(crate) fn restore_plan_strict(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<Option<ChapterDraftRestorePlan>, String> {
    let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
        return Ok(None);
    };
    // 恢复前先补做 pending-ack，保证草稿 base 与 Core journal 一致。
    super::replay_pending_acks(projects_root, project_id, chapter_id)?;
    let mut state = match store::read_new_state(&location, false)? {
        Some(state) => state,
        // 迁移失败必须作为 Err 上报（保留旧文件与新目录原件），不能降级成
        // “没有草稿”被恢复链当成空草稿清理。
        None => match store::migrate_legacy(&location)? {
            Some(state) => state,
            None => return Ok(None),
        },
    };
    // Issue #1062 评论 7193847871 第 2 项：meta 的 committedText* 是正文已
    // 收口的 durable 决议。已确认快照对应的 text.txt（删除残留或缺失）都不能
    // 再作为待恢复正文；否则一次 meta 成功、文件删除失败的崩溃会让恢复报
    // “text missing” 永久卡住。
    let committed_text = !state.committed_text_sha256.is_empty()
        && state.text_revision == state.committed_text_revision
        && state.text_sha256 == state.committed_text_sha256;
    if let Some(text) = state.text.as_ref() {
        if !state.text_sha256.is_empty() {
            let actual = writer_core::writing_stats::proof::content_digest_of_text(text);
            if actual != state.text_sha256 {
                return Err(format!(
                    "draft text corrupt (kept): {project_id}/{chapter_id} expected={} actual={actual}",
                    state.text_sha256
                ));
            }
        }
    } else if (state.text_revision > 0 || !state.text_sha256.is_empty()) && !committed_text {
        return Err(format!(
            "chapter draft text missing (kept): project={project_id} chapter={chapter_id} text_revision={}",
            state.text_revision
        ));
    }
    if committed_text {
        // 已确认快照的残留 text.txt 不作为待恢复正文；没有 text.txt 时也不
        // 按缺失报错（meta 决议说明正文已由 Core 提交）。
        state.text = None;
    }
    let mut values = Vec::with_capacity(state.facts.len());
    let mut loaded: Vec<LoadedFact> = Vec::with_capacity(state.facts.len());
    for fact in &state.facts {
        let value = read_projection(&location, fact).map_err(|error| {
            format!("draft fact unreadable (kept): {project_id}/{chapter_id}: {error}")
        })?;
        loaded.push(LoadedFact {
            event_id: fact.event_id.clone(),
            revision: fact.revision,
            projection_json: value.to_string(),
        });
        values.push(value);
    }
    let batch_json = serde_json::to_string(&values).map_err(|error| {
        format!("draft batch encode failed: {project_id}/{chapter_id}: {error}")
    })?;
    let sessions: HashSet<u64> = state.facts.iter().map(|fact| fact.session_id).collect();
    Ok(Some(ChapterDraftRestorePlan {
        volume_id: state.volume_id.clone(),
        chapter_id: state.chapter_id.clone(),
        base_generation: state.base_generation.clone(),
        base_revision: state.base_revision,
        base_sha256: state.base_sha256.clone(),
        text: state.text.as_deref().map(str::to_string),
        text_revision: state.text_revision,
        text_sha256: state.text_sha256.clone(),
        fact_count: state.facts.len(),
        event_end_revision: loaded.last().map_or(0, |fact| fact.revision),
        editor_base_revision: state.editor_base_revision,
        editor_base_sha256: state.editor_base_sha256.clone(),
        committed_text_revision: state.committed_text_revision,
        committed_text_sha256: state.committed_text_sha256.clone(),
        batch_json,
        facts: loaded,
        session_id: state.session_id,
        generation: state.generation.clone(),
        mixed_sessions: sessions.len() > 1,
    }))
}

/// 兼容入口：调用方只关心“是否有可恢复计划”。
pub(crate) fn restore_plan(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<ChapterDraftRestorePlan> {
    restore_plan_strict(projects_root, project_id, chapter_id)
        .map_err(|error| {
            log::warn!(
                "chapter draft restore plan unavailable (kept): {project_id}/{chapter_id}: {error}"
            );
        })
        .ok()
        .flatten()
}

/// 内存占用：内存索引事实条数、正文总字节数（不含 projection 历史）。

fn read_projection(
    location: &DraftLocation,
    fact: &DraftFactIndex,
) -> Result<serde_json::Value, String> {
    let path = store::fact_file_path(&location.dir, fact.seq, &fact.event_id);
    let parsed = store::read_fact_file(&path)?;
    if parsed.event_id != fact.event_id || parsed.revision != fact.revision {
        return Err(format!(
            "draft fact mismatch event={} path={}",
            fact.event_id,
            path.display()
        ));
    }
    serde_json::from_str(&parsed.projection_json).map_err(|error| {
        format!(
            "draft fact projection invalid event={}: {error}",
            fact.event_id
        )
    })
}
