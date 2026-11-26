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
    if let Some(state) = store::read_new_state(&location, false) {
        return Some(ChapterDraftSnapshot::from_state(&state));
    }
    let legacy = store::read_legacy(&location.legacy_path(), project_id, chapter_id)?;
    Some(ChapterDraftSnapshot::from_state(
        &store::legacy_state(&location, legacy).0,
    ))
}

/// 列出项目内所有需要恢复的草稿（有事实或有正文）。
pub(crate) fn list_project_drafts(projects_root: &Path, project_id: &str) -> Vec<ChapterDraft> {
    if !store::is_safe_id(project_id) {
        return Vec::new();
    }
    let workspace = store::workspace_key_of(projects_root);
    let root = store::drafts_root(projects_root, project_id);
    let mut drafts: Vec<ChapterDraft> = {
        let registry = lock_registry();
        registry
            .values()
            .filter(|state| state.project_id == project_id && state.workspace_key == workspace)
            .map(DraftState::summary)
            .collect()
    };
    drafts.sort_by(|left, right| left.chapter_id.cmp(&right.chapter_id));
    let Ok(entries) = std::fs::read_dir(&root) else {
        return drafts;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Some(chapter_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !store::is_safe_id(chapter_id)
                || drafts.iter().any(|draft| draft.chapter_id == chapter_id)
            {
                continue;
            }
            if let Some(summary) =
                store::read_summary_from_dir(projects_root, &path, project_id, chapter_id)
            {
                drafts.push(summary);
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
            || drafts.iter().any(|draft| draft.chapter_id == chapter_id)
        {
            continue;
        }
        if let Some(legacy) = store::read_legacy(&path, project_id, chapter_id) {
            let Some(location) = DraftLocation::new(projects_root, project_id, chapter_id) else {
                continue;
            };
            drafts.push(store::legacy_state(&location, legacy).0.summary());
        }
    }
    drafts
}

/// 读取草稿的恢复计划；任何文件缺失/损坏/正文摘要不一致都返回 None，
/// 且不删除任何草稿文件（保留给下一次重试或人工排查）。
pub(crate) fn restore_plan(
    projects_root: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Option<ChapterDraftRestorePlan> {
    let location = DraftLocation::new(projects_root, project_id, chapter_id)?;
    let state =
        store::read_new_state(&location, false).or_else(|| store::migrate_legacy(&location))?;
    if let Some(text) = state.text.as_ref() {
        if !state.text_sha256.is_empty() {
            let actual = writer_core::writing_stats::proof::content_digest_of_text(text);
            if actual != state.text_sha256 {
                log::warn!(
                    "draft text corrupt: {project_id}/{chapter_id} expected={} actual={actual}",
                    state.text_sha256
                );
                return None;
            }
        }
    } else if state.text_revision > 0 || !state.text_sha256.is_empty() {
        log::warn!(
            "chapter draft text missing: project={project_id} chapter={chapter_id} text_revision={}",
            state.text_revision
        );
        return None;
    }
    let mut values = Vec::with_capacity(state.facts.len());
    let mut loaded: Vec<LoadedFact> = Vec::with_capacity(state.facts.len());
    for fact in &state.facts {
        match read_projection(&location, fact) {
            Ok(value) => {
                loaded.push(LoadedFact {
                    event_id: fact.event_id.clone(),
                    revision: fact.revision,
                    projection_json: value.to_string(),
                });
                values.push(value);
            }
            Err(error) => {
                log::warn!("draft fact unreadable: {project_id}/{chapter_id}: {error}");
                return None;
            }
        }
    }
    let batch_json = match serde_json::to_string(&values) {
        Ok(json) => json,
        Err(error) => {
            log::warn!("draft batch encode failed: {project_id}/{chapter_id}: {error}");
            return None;
        }
    };
    let sessions: HashSet<u64> = state.facts.iter().map(|fact| fact.session_id).collect();
    Some(ChapterDraftRestorePlan {
        volume_id: state.volume_id.clone(),
        chapter_id: state.chapter_id.clone(),
        base_generation: state.base_generation.clone(),
        base_revision: state.base_revision,
        base_sha256: state.base_sha256.clone(),
        text: state.text.clone(),
        text_revision: state.text_revision,
        text_sha256: state.text_sha256.clone(),
        fact_count: state.facts.len(),
        event_end_revision: loaded.last().map_or(0, |fact| fact.revision),
        batch_json,
        facts: loaded,
        session_id: state.session_id,
        generation: state.generation.clone(),
        mixed_sessions: sessions.len() > 1,
    })
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
