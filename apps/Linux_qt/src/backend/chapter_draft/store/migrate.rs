// =============================================================================
// chapter_draft/store/migrate.rs — 旧格式单文件迁移与只读解码
// =============================================================================
//
// Issue #1062 评论 #7213284979 第 2 项：迁移先写到独立 staging 目录，全部
// durable 后才原子安装；目标活动目录里的任何真实内容都整体移入归档区保留，
// 绝不 remove_dir_all。失败返回 Err，旧文件与新目录原件都保留。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::DraftState;
use super::{DraftFactIndex, DraftLocation};

/// 旧格式单文件迁移：在独立 staging 目录写完 facts/正文/meta 后原子安装。
///
/// Issue #1062 评论 #7213284979 第 2 项：目标活动目录里的任何真实内容都
/// 不能先 `remove_dir_all`（那里可能已有独立未提交事实、spool 或 pending
/// ack）。迁移失败必须返回 Err，旧单文件与新目录原始字节都保留。
pub(super) fn migrate_legacy(location: &DraftLocation) -> Result<Option<DraftState>, String> {
    let legacy_path = location.legacy_path();
    let Some(legacy) =
        read_legacy_checked(&legacy_path, &location.project_id, &location.chapter_id)?
    else {
        return Ok(None);
    };
    let (mut state, projections) = legacy_state(location, legacy);
    let expected_event_ids: Vec<String> = state
        .facts
        .iter()
        .map(|fact| fact.event_id.clone())
        .collect();
    let staging = migration_staging_dir(location);
    if let Some(parent) = staging.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "create chapter draft migration staging parent {}: {error}",
                parent.display()
            )
        })?;
    }
    std::fs::create_dir(&staging).map_err(|error| {
        format!(
            "create chapter draft migration staging {}: {error}",
            staging.display()
        )
    })?;
    for (fact, projection) in state.facts.iter().zip(projections.iter()) {
        super::write_fact_file(
            &staging,
            fact.seq,
            &fact.event_id,
            fact.revision,
            fact.session_id,
            projection,
        )
        .map_err(|error| {
            format!(
                "migrate legacy chapter draft fact failed: project={} chapter={} event={}: {error}",
                location.project_id, location.chapter_id, fact.event_id
            )
        })?;
    }
    let mut text_blob = String::new();
    if let Some(text) = state.text.as_deref() {
        text_blob = super::write_text_blob(&staging, state.text_revision, &state.text_sha256, text)
            .map_err(|error| {
                format!(
                    "migrate legacy chapter draft text failed: project={} chapter={}: {error}",
                    location.project_id, location.chapter_id
                )
            })?;
    }
    state.persisted_text_blob = text_blob.clone();
    let meta = super::meta_from_state(
        location,
        &state,
        state.text_revision,
        &state.text_sha256,
        &text_blob,
    );
    super::write_meta(&staging, &meta).map_err(|error| {
        format!(
            "migrate legacy chapter draft meta failed: project={} chapter={}: {error}",
            location.project_id, location.chapter_id
        )
    })?;
    install_migration(location, &staging)?;
    let state = super::read_new_state(location, false)?.ok_or_else(|| {
        format!(
            "migrated chapter draft has no readable meta (kept): project={} chapter={}",
            location.project_id, location.chapter_id
        )
    })?;
    // 只有全量事实都已 durable 且能被读回，才允许接下来删除旧单文件。
    for event_id in &expected_event_ids {
        if !state.facts.iter().any(|fact| &fact.event_id == event_id) {
            return Err(format!(
                "migrated chapter draft lost fact (kept): event={event_id} project={} chapter={}",
                location.project_id, location.chapter_id
            ));
        }
    }
    if let Err(error) = std::fs::remove_file(&legacy_path) {
        log::warn!(
            "remove migrated legacy chapter draft failed: project={} chapter={} path={}: {error}",
            location.project_id,
            location.chapter_id,
            legacy_path.display()
        );
    }
    log::info!(
        "migrated legacy chapter draft: project={} chapter={} facts={}",
        location.project_id,
        location.chapter_id,
        state.facts.len()
    );
    Ok(Some(state))
}

fn migration_staging_dir(location: &DraftLocation) -> PathBuf {
    let root = location
        .projects_root
        .join(&location.project_id)
        .join("app-meta")
        .join("chapter-draft-migrating")
        .join(&location.chapter_id);
    let mut attempt = 0u32;
    loop {
        let path = root.join(format!(
            "{}-{}-{attempt}",
            super::super::now_ms(),
            std::process::id()
        ));
        if !path.exists() || attempt >= 999 {
            return path;
        }
        attempt = attempt.saturating_add(1);
    }
}

/// 把迁移 staging 原子安装到活动槽位；已有目录内容整体移入归档区保留。
fn install_migration(location: &DraftLocation, staging: &Path) -> Result<(), String> {
    if location.dir.exists() {
        if super::dir_has_any_entry(&location.dir) {
            // Issue #1062 评论 #7213499162 第 1 项：这份新版草稿不能被当成
            // 无身份的孤儿归档。先写入 durable 迁移冲突意图，随目录一起
            // rename 进 kept，恢复 actor 才能在之后继续按事务重试它。
            let volume_id =
                super::read_meta_checked(&location.dir, &location.project_id, &location.chapter_id)
                    .map(|meta| meta.volume_id)
                    .unwrap_or_default();
            let reason = format!(
                "legacy_migration_conflict: newer draft archived while migrating legacy file {}",
                location.legacy_path().display()
            );
            super::super::retry::write_retry_intent(
                &location.projects_root,
                &location.project_id,
                &location.chapter_id,
                &volume_id,
                &reason,
                0,
            )
            .map_err(|error| {
                format!(
                    "write legacy migration conflict intent for project={} chapter={}: {error}",
                    location.project_id, location.chapter_id
                )
            })?;
            let archive = super::kept_root(&location.projects_root, &location.project_id)
                .join(&location.chapter_id)
                .join(format!(
                    "legacy-merge-{}-{}",
                    super::super::now_ms(),
                    std::process::id()
                ));
            if let Some(parent) = archive.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "create legacy migration archive {}: {error}",
                        parent.display()
                    )
                })?;
            }
            std::fs::rename(&location.dir, &archive).map_err(|error| {
                format!(
                    "archive existing draft dir before legacy migration {} -> {}: {error}",
                    location.dir.display(),
                    archive.display()
                )
            })?;
            log::warn!(
                "legacy migration archived existing draft content (kept): project={} chapter={} path={}",
                location.project_id,
                location.chapter_id,
                archive.display()
            );
        } else {
            std::fs::remove_dir(&location.dir).map_err(|error| {
                format!(
                    "remove empty draft dir before legacy migration {}: {error}",
                    location.dir.display()
                )
            })?;
        }
    }
    if let Some(parent) = location.dir.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "create draft dir {} before legacy migration: {error}",
                parent.display()
            )
        })?;
    }
    std::fs::rename(staging, &location.dir).map_err(|error| {
        format!(
            "install migrated draft {} -> {}: {error}",
            staging.display(),
            location.dir.display()
        )
    })
}

// ── 旧格式 <chapter_id>.json ──────────────────────────────────────────────

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyFile {
    project_id: String,
    volume_id: String,
    chapter_id: String,
    base_generation: String,
    base_revision: u64,
    base_sha256: String,
    text: Option<String>,
    text_revision: u64,
    text_sha256: String,
    updated_at_ms: i64,
    facts: Vec<LegacyFact>,
}

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyFact {
    event_id: String,
    revision: u64,
    projection: serde_json::Value,
}

pub(super) struct LegacyData {
    pub(super) volume_id: String,
    pub(super) base_generation: String,
    pub(super) base_revision: u64,
    pub(super) base_sha256: String,
    pub(super) text: Option<String>,
    pub(super) text_revision: u64,
    pub(super) text_sha256: String,
    pub(super) updated_at_ms: i64,
    /// (索引, projection JSON)
    pub(super) facts: Vec<(DraftFactIndex, String)>,
}

pub(super) fn read_legacy(path: &Path, project_id: &str, chapter_id: &str) -> Option<LegacyData> {
    match read_legacy_checked(path, project_id, chapter_id) {
        Ok(legacy) => legacy,
        Err(error) => {
            log::warn!("{error}");
            None
        }
    }
}

/// 严格读取旧格式单文件：区分“不存在”与“存在但读不出/身份不符”。
pub(super) fn read_legacy_checked(
    path: &Path,
    project_id: &str,
    chapter_id: &str,
) -> Result<Option<LegacyData>, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "read legacy chapter draft {}: {error}",
                path.display()
            ))
        }
    };
    let file: LegacyFile = serde_json::from_str(&content).map_err(|error| {
        format!(
            "legacy chapter draft invalid (kept): path={}: {error}",
            path.display()
        )
    })?;
    if file.project_id != project_id || file.chapter_id != chapter_id {
        return Err(format!(
            "legacy chapter draft identity mismatch (kept): project={project_id} chapter={chapter_id} path={}",
            path.display()
        ));
    }
    // Issue #1062 评论 #7213499162 第 2 项：旧格式事实不能静默过滤。每一条
    // 都必须可转换（非空事件 ID、对象 projection、projection 身份一致且不
    // 重复）；任何一条不合法都返回 Err，源文件保留。
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (index, item) in file.facts.iter().enumerate() {
        if item.event_id.is_empty() {
            return Err(format!(
                "legacy chapter draft fact {index} has empty event id (kept): path={}",
                path.display()
            ));
        }
        if !item.projection.is_object() {
            return Err(format!(
                "legacy chapter draft fact {index} projection invalid (kept): event={} path={}",
                item.event_id,
                path.display()
            ));
        }
        if let Some(projection_event_id) = item
            .projection
            .get("eventId")
            .and_then(|value| value.as_str())
        {
            if projection_event_id != item.event_id {
                return Err(format!(
                    "legacy chapter draft fact {index} identity mismatch (kept): event={} projectionEvent={projection_event_id} path={}",
                    item.event_id,
                    path.display()
                ));
            }
        }
        if !seen.insert(item.event_id.as_str()) {
            return Err(format!(
                "legacy chapter draft duplicate event id (kept): event={} path={}",
                item.event_id,
                path.display()
            ));
        }
    }
    let mut facts = Vec::with_capacity(file.facts.len());
    for item in file.facts {
        let seq = u64::try_from(facts.len()).unwrap_or(0).saturating_add(1);
        facts.push((
            DraftFactIndex {
                event_id: item.event_id,
                revision: item.revision,
                session_id: 0,
                after_text_sha256: super::super::projection_after_text_sha256(
                    &item.projection.to_string(),
                ),
                seq,
            },
            item.projection.to_string(),
        ));
    }
    Ok(Some(LegacyData {
        volume_id: file.volume_id,
        base_generation: file.base_generation,
        base_revision: file.base_revision,
        base_sha256: file.base_sha256,
        text: file.text,
        text_revision: file.text_revision,
        text_sha256: file.text_sha256,
        updated_at_ms: file.updated_at_ms,
        facts,
    }))
}

/// 旧格式只读解码结果（快照/列表用，不写盘）。
pub(super) fn legacy_state(
    location: &DraftLocation,
    legacy: LegacyData,
) -> (DraftState, Vec<String>) {
    let mut facts = Vec::with_capacity(legacy.facts.len());
    let mut projections = Vec::with_capacity(legacy.facts.len());
    for (fact, projection) in legacy.facts {
        facts.push(fact);
        projections.push(projection);
    }
    let next_seq = facts.last().map_or(1, |fact| fact.seq.saturating_add(1));
    let state = DraftState {
        project_id: location.project_id.clone(),
        volume_id: legacy.volume_id,
        chapter_id: location.chapter_id.clone(),
        workspace_key: location.workspace_key.clone(),
        generation: String::new(),
        session_id: 0,
        base_generation: legacy.base_generation,
        base_revision: legacy.base_revision,
        base_sha256: legacy.base_sha256,
        text: legacy.text.map(Arc::from),
        text_revision: legacy.text_revision,
        text_sha256: legacy.text_sha256.clone(),
        persisted_text_revision: legacy.text_revision,
        persisted_text_sha256: legacy.text_sha256,
        persisted_text_blob: String::new(),
        text_dirty: false,
        facts,
        next_seq,
        failed_facts: Vec::new(),
        failed_inputs: Vec::new(),
        acked_event_ids: Vec::new(),
        applied_transactions: Vec::new(),
        applied_commit_sequence: 0,
        editor_base_revision: 0,
        editor_base_sha256: String::new(),
        editor_base_session: 0,
        committed_text_revision: 0,
        committed_text_sha256: String::new(),
        acked_through_revision: 0,
        projects_root: location.projects_root.clone(),
        updated_at_ms: legacy.updated_at_ms,
        meta_dirty: false,
        last_persist: None,
    };
    (state, projections)
}
