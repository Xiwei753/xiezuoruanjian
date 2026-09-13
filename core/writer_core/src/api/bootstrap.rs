//! 应用服务启动入口。
//!
//! 提供从平台初始化信息构造 `WriterAppService` 的若干变体：
//! - `open_app_service`：仅传入根目录，不注入平台服务
//! - `open_app_service_with_init`：注入 `PlatformInitDto`（含网络状态）
//! - `open_app_service_with_secure_storage`：同时注入安全存储 callback
//!
//! 这些函数是平台客户端打开 Core 的统一入口，UniFFI 通过 `#[::uniffi::export]`
//! 暴露给各平台绑定层。

use std::path::Path;
use std::sync::Arc;

use writer_platform_api::{
    get_platform_services_resolver, ConfigStore, FileConfigStore, NetworkState, PlatformInit,
    PlatformServices, SecureStorage,
};

use super::secure_storage_bridge::{wrap_secure_storage, SecureStorageProvider};
use crate::api::error::WriterError;
use crate::api::service::WriterCoreApi;
use crate::api::types::PlatformInitDto;
use crate::app_service::WriterAppService;

///   启动时恢复待处理的删除事务。
///
/// 在创建 `WriterAppService` 之前调用，确保崩溃前的删除事务被完成。
/// 恢复失败返回 Err（用 `?` 严格返回），让调用方决定。
///
/// recover 返回 `Vec<RecoveredProjectDelete>`，
/// 每个含待补 history 的 change-set。bootstrap 用 layout 调
/// `record_workspace_change_set` 写本地 history，成功后调
/// `ack_project_delete_history` 推进 journal 到 `HistoryRecorded` → `Completed`
/// 并清 journal。history 失败时 log::warn 并保留 journal，下次启动 recover 补记。
fn recover_storage_transactions(
    app_data_root: &Path,
    layout: &crate::storage::git_repo_layout::GitRepoLayout,
) -> std::result::Result<(), WriterError> {
    // 恢复项目删除事务（project_delete journal）。
    let recovered_list =
        crate::storage::journal::project_delete::recover_pending_delete_transactions(
            app_data_root,
        )?;
    for rec in &recovered_list {
        match crate::storage::workspace_git::record_workspace_change_set(
            layout,
            &rec.changes,
            "recover_project_delete",
        ) {
            Ok(result) => {
                if result.oid.is_some() {
                    log::debug!(
                        "recover_storage_transactions: history committed for {} ({} staged)",
                        rec.journal_token,
                        result.staged_count
                    );
                }
                // history 成功，ack 推进 journal。
                if let Err(e) = crate::storage::journal::project_delete::ack_project_delete_history(
                    app_data_root,
                    &rec.journal_token,
                ) {
                    log::warn!(
                        "recover_storage_transactions: ack failed for {}: {} — journal retained",
                        rec.journal_token,
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "recover_storage_transactions: record_workspace_change_set failed for {}: {} \
                     — journal retained, history will be补 on next startup",
                    rec.journal_token,
                    e
                );
            }
        }
    }

    // 恢复统一 workspace 变更事务（workspace_change journal）。
    recover_workspace_change_transactions(app_data_root, layout)?;
    Ok(())
}

/// 恢复统一 workspace 变更事务（workspace_change journal）。
///
/// 按阶段区分处理：
/// - `Pending`: journal 落盘但本地删除未完成。先幂等完成本地删除（目标已不存在视为已完成），
///   推进到 LocalApplied，再补 history。
/// - `LocalApplied`: 本地删除已完成，补 history，推进到 HistoryRecorded，清 journal。
/// - `HistoryRecorded`: 已在 recover_unfinished 内部清理。
///
/// 不再让 Pending 直接进入 history——必须先完成本地删除。
fn recover_workspace_change_transactions(
    app_data_root: &Path,
    layout: &crate::storage::git_repo_layout::GitRepoLayout,
) -> std::result::Result<(), WriterError> {
    let recovered_changes =
        crate::storage::journal::workspace_change::recover_unfinished(app_data_root)?;
    for rec in &recovered_changes {
        let message = recover_message_for_op_type(&rec.op_type);

        // 阶段 1：如果是 Pending，先幂等完成本地删除。
        let should_continue = recover_pending_phase(app_data_root, rec);
        if !should_continue {
            continue;
        }

        // 阶段 2：LocalApplied，补 history。
        recover_local_applied_phase(app_data_root, layout, rec, message);
    }
    Ok(())
}

/// 根据 op_type 返回 recover 用的 commit message。
fn recover_message_for_op_type(
    op_type: &crate::storage::journal::workspace_change::WorkspaceChangeOpType,
) -> &'static str {
    use crate::storage::journal::workspace_change::WorkspaceChangeOpType;
    match op_type {
        WorkspaceChangeOpType::DeleteProject => "recover_delete_project",
        WorkspaceChangeOpType::DeleteVolume => "recover_delete_volume",
        WorkspaceChangeOpType::DeleteChapter => "recover_delete_chapter",
    }
}

/// 处理 Pending 阶段：幂等完成本地删除并推进 journal 到 LocalApplied。
///
/// 返回 `true` 表示可以继续补 history（已推进到 LocalApplied 或本来就是 LocalApplied）。
/// 返回 `false` 表示应跳过本次循环（本地删除失败或阶段非 LocalApplied）。
fn recover_pending_phase(
    app_data_root: &Path,
    rec: &crate::storage::journal::workspace_change::RecoveredWorkspaceChange,
) -> bool {
    use crate::storage::journal::workspace_change::WorkspaceChangePhase;

    if rec.phase != WorkspaceChangePhase::Pending {
        // 非 Pending 阶段，直接检查是否 LocalApplied。
        return rec.phase == WorkspaceChangePhase::LocalApplied;
    }

    // Pending 阶段：先幂等完成本地删除。
    match recover_pending_local_delete(app_data_root, rec) {
        Ok(()) => {
            // 本地删除完成，推进 journal 到 LocalApplied。
            //   ：保留原 journal 的 device_id/created_at/sync_delete_facts/planned_delete，
            // 不用空 device_id、created_at=0 覆盖原事务元数据。
            let journal = crate::storage::journal::workspace_change::WorkspaceChangeJournal {
                token: rec.journal_token.clone(),
                change_set: rec.changes.clone(),
                device_id: rec.device_id.clone(),
                op_type: rec.op_type.clone(),
                created_at: rec.created_at,
                phase: WorkspaceChangePhase::Pending,
                delete_target: rec.delete_target.clone(),
                sync_delete_facts: rec.sync_delete_facts.clone(),
                planned_delete: rec.planned_delete.clone(),
            };
            if let Err(e) = journal.mark_local_applied(app_data_root) {
                log::warn!(
                    "recover_workspace_change_transactions: mark_local_applied failed \
                     for {}: {} — journal retained",
                    rec.journal_token,
                    e
                );
            }
            true
        }
        Err(e) => {
            log::warn!(
                "recover_workspace_change_transactions: pending local delete failed for \
                 {}: {} — journal retained, will retry on next startup",
                rec.journal_token,
                e
            );
            false
        }
    }
}

/// 处理 LocalApplied 阶段：补 history，推进到 HistoryRecorded，清 journal。
fn recover_local_applied_phase(
    app_data_root: &Path,
    layout: &crate::storage::git_repo_layout::GitRepoLayout,
    rec: &crate::storage::journal::workspace_change::RecoveredWorkspaceChange,
    message: &str,
) {
    match crate::storage::workspace_git::record_workspace_change_set(layout, &rec.changes, message)
    {
        Ok(result) => {
            if result.oid.is_some() {
                log::debug!(
                    "recover_workspace_change_transactions: history committed for {} \
                     ({} staged)",
                    rec.journal_token,
                    result.staged_count
                );
            }
            // history 成功，推进 journal 到 HistoryRecorded 并清理。
            //   ：保留原 journal 的 device_id/created_at/sync_delete_facts/planned_delete，
            // 不用空 device_id、created_at=0 覆盖原事务元数据。
            let journal = crate::storage::journal::workspace_change::WorkspaceChangeJournal {
                token: rec.journal_token.clone(),
                change_set: rec.changes.clone(),
                device_id: rec.device_id.clone(),
                op_type: rec.op_type.clone(),
                created_at: rec.created_at,
                phase:
                    crate::storage::journal::workspace_change::WorkspaceChangePhase::LocalApplied,
                delete_target: rec.delete_target.clone(),
                sync_delete_facts: rec.sync_delete_facts.clone(),
                planned_delete: rec.planned_delete.clone(),
            };
            if let Err(e) = journal.mark_history_recorded(app_data_root) {
                log::warn!(
                    "recover_workspace_change_transactions: mark_history_recorded failed \
                     for {}: {} — journal retained",
                    rec.journal_token,
                    e
                );
            } else if let Err(e) = journal.clear_journal(app_data_root) {
                log::warn!(
                    "recover_workspace_change_transactions: clear_journal failed for {}: {}",
                    rec.journal_token,
                    e
                );
            }
        }
        Err(e) => {
            log::warn!(
                "recover_workspace_change_transactions: history failed for {}: {} \
                 — journal retained, history will be补 on next startup",
                rec.journal_token,
                e
            );
        }
    }
}

/// 源目录还在时重放 Volume 删除：有 `planned_delete` 直接 apply，无则先升级 journal 再 apply。
///
/// 旧格式 journal（无 `planned_delete`）走"重新 plan + 回写 durable Pending + 统一 apply"，
/// 不再调旧 `delete_volume`，确保 trash path / tombstone 落盘顺序符合 durable 协议。
fn apply_volume_delete_or_upgrade(
    app_data_root: &Path,
    rec: &crate::storage::journal::workspace_change::RecoveredWorkspaceChange,
    project_root: &Path,
    volume_id: &str,
    volume_dir: &Path,
) -> std::result::Result<(), WriterError> {
    if let Some(planned) = &rec.planned_delete {
        crate::volume::apply_planned_delete_volume(project_root, volume_id, app_data_root, planned)
            .map_err(WriterError::from)
    } else {
        log::debug!(
            "recover_pending_local_delete: old-format journal, upgrading plan for {}",
            volume_dir.display()
        );
        let (_new_change_set, planned) = crate::volume::plan_delete_volume(
            project_root,
            volume_id,
            app_data_root,
            &rec.device_id,
        )
        .map_err(WriterError::from)?;
        crate::storage::journal::workspace_change::upgrade_pending_plan(
            app_data_root,
            rec,
            planned.clone(),
        )
        .map_err(WriterError::from)?;
        crate::volume::apply_planned_delete_volume(project_root, volume_id, app_data_root, &planned)
            .map_err(WriterError::from)
    }
}

/// 源目录还在时重放 Chapter 删除：有 `planned_delete` 直接 apply，无则先升级 journal 再 apply。
///
/// 旧格式 journal（无 `planned_delete`）走"重新 plan + 回写 durable Pending + 统一 apply"，
/// 不再调旧 `delete_chapter`，确保 trash path / tombstone 落盘顺序符合 durable 协议。
fn apply_chapter_delete_or_upgrade(
    app_data_root: &Path,
    rec: &crate::storage::journal::workspace_change::RecoveredWorkspaceChange,
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    chapter_dir: &Path,
) -> std::result::Result<(), WriterError> {
    if let Some(planned) = &rec.planned_delete {
        crate::chapter::apply_planned_delete_chapter(
            project_root,
            volume_id,
            chapter_id,
            app_data_root,
            planned,
        )
        .map_err(WriterError::from)
    } else {
        log::debug!(
            "recover_pending_local_delete: old-format journal, upgrading plan for {}",
            chapter_dir.display()
        );
        let (_new_change_set, planned) = crate::chapter::plan_delete_chapter(
            project_root,
            volume_id,
            chapter_id,
            app_data_root,
            app_data_root, // workspace_root == app_data_root
            &rec.device_id,
        )
        .map_err(WriterError::from)?;
        crate::storage::journal::workspace_change::upgrade_pending_plan(
            app_data_root,
            rec,
            planned.clone(),
        )
        .map_err(WriterError::from)?;
        crate::chapter::apply_planned_delete_chapter(
            project_root,
            volume_id,
            chapter_id,
            app_data_root,
            &planned,
        )
        .map_err(WriterError::from)
    }
}

/// 幂等完成 Pending 阶段的本地删除。
///
/// 重放同一个 planned delete：
/// - 源目录还在：按 journal 固定的 trash path 完成 rename，再写 facts。
/// - 源目录已经不在：仍然必须根据 journal facts 补齐 project tombstone，
///   成功后才能推进 `LocalApplied`。
///
/// 对新格式 `DeleteVolume/DeleteChapter`（`planned_delete` 非空）：
/// - `sync_delete_facts.is_empty()` 是不可完成的事务——保留 journal 并返回恢复错误，
///   绝不能继续写 history/清 journal。
///
/// 对旧格式 journal（`planned_delete` 为空，向后兼容）：
/// - 如果源目录仍存在，先原地升级 journal（重新 plan + 回写 durable Pending），
///   再走统一 `apply_planned_delete_*`，不再调旧 `delete_volume`/`delete_chapter`。
/// - 如果源目录已经消失又没有任何 durable facts，就不要凭空伪造远端 delete——
///   返回恢复错误，保留 journal。
fn recover_pending_local_delete(
    app_data_root: &Path,
    rec: &crate::storage::journal::workspace_change::RecoveredWorkspaceChange,
) -> std::result::Result<(), WriterError> {
    use crate::storage::journal::workspace_change::DeleteTarget;
    let projects_root = app_data_root.join("projects");
    match &rec.delete_target {
        Some(DeleteTarget::Volume {
            project_id,
            volume_id,
        }) => {
            let project_root = projects_root.join(project_id);
            let volume_dir = project_root.join("volumes").join(volume_id);
            if !volume_dir.exists() {
                log::debug!(
                    "recover_pending_local_delete: volume {} already absent — ensuring tombstones persisted",
                    volume_dir.display()
                );
                // 目录已不存在，但 tombstone 可能没落盘。根据 sync_delete_facts 幂等补齐。
                ensure_tombstones_persisted(&project_root, &rec.sync_delete_facts)?;
                return Ok(());
            }
            // 源目录还在：重放 planned delete（或旧格式升级后 apply）。
            apply_volume_delete_or_upgrade(
                app_data_root,
                rec,
                &project_root,
                volume_id,
                &volume_dir,
            )
        }
        Some(DeleteTarget::Chapter {
            project_id,
            volume_id,
            chapter_id,
        }) => {
            let project_root = projects_root.join(project_id);
            let chapter_dir = project_root
                .join("volumes")
                .join(volume_id)
                .join("chapters")
                .join(chapter_id);
            if !chapter_dir.exists() {
                log::debug!(
                    "recover_pending_local_delete: chapter {} already absent — ensuring tombstones persisted",
                    chapter_dir.display()
                );
                // 目录已不存在，但 tombstone 可能没落盘。根据 sync_delete_facts 幂等补齐。
                ensure_tombstones_persisted(&project_root, &rec.sync_delete_facts)?;
                return Ok(());
            }
            // 源目录还在：重放 planned delete（或旧格式升级后 apply）。
            apply_chapter_delete_or_upgrade(
                app_data_root,
                rec,
                &project_root,
                volume_id,
                chapter_id,
                &chapter_dir,
            )
        }
        Some(DeleteTarget::Project { project_id }) => {
            // 作品删除有独立的多阶段事务（project_delete.rs），不在此处理。
            // Pending 阶段的 DeleteProject 不应出现在 workspace_change journal 里。
            // fail-closed：返回错误，保留 journal，不能继续写 history。
            Err(WriterError::Other(format!(
                "recover_pending_local_delete: DeleteProject target {} in generic workspace_change \
                 journal {} — should use project_delete journal; journal retained",
                project_id, rec.journal_token
            )))
        }
        None => {
            // 旧 journal 无 delete_target，尝试从 change_set 无歧义迁移。
            // 如果无法无歧义地确定 delete_target，返回错误保留 journal。
            recover_pending_local_delete_no_target(app_data_root, rec)
        }
    }
}

/// 处理旧格式 journal（无 delete_target）的 Pending 阶段恢复。
///
/// 尝试从 change_set 无歧义地迁移 delete_target：
/// - `DeleteVolume`：从 `DeleteTree(projects/{pid}/volumes/{vid})` 提取。
/// - `DeleteChapter`：从 `Delete(projects/{pid}/volumes/{vid}/chapters/{cid}/chapter.*)` 提取。
/// - `DeleteProject`：不应出现在 generic journal 中，返回错误。
///
/// 迁移成功后，源目录仍存在时先原地升级 journal（重新 plan + 回写 durable Pending），
/// 再走统一 `apply_planned_delete_*`，不再调旧 `delete_volume`/`delete_chapter`；
/// 源目录已消失时返回错误（旧格式没有 `sync_delete_facts`，无法补齐 tombstone）。
fn recover_pending_local_delete_no_target(
    app_data_root: &Path,
    rec: &crate::storage::journal::workspace_change::RecoveredWorkspaceChange,
) -> std::result::Result<(), WriterError> {
    use crate::storage::journal::workspace_change::WorkspaceChangeOpType;
    let projects_root = app_data_root.join("projects");
    match &rec.op_type {
        WorkspaceChangeOpType::DeleteVolume => {
            let (project_id, volume_id) = migrate_volume_target_from_change_set(&rec.changes)
                .ok_or_else(|| {
                    WriterError::Other(format!(
                        "recover_pending_local_delete: cannot unambiguously migrate \
                         delete_target from change_set for journal {} — journal retained",
                        rec.journal_token
                    ))
                })?;
            let project_root = projects_root.join(&project_id);
            let volume_dir = project_root.join("volumes").join(&volume_id);
            if !volume_dir.exists() {
                return Err(WriterError::Other(format!(
                    "recover_pending_local_delete: old-format journal {} — volume {} \
                     already absent but no sync_delete_facts to ensure tombstones; \
                     journal retained",
                    rec.journal_token,
                    volume_dir.display()
                )));
            }
            log::debug!(
                "recover_pending_local_delete: migrated delete_target from change_set for {}",
                volume_dir.display()
            );
            apply_volume_delete_or_upgrade(
                app_data_root,
                rec,
                &project_root,
                &volume_id,
                &volume_dir,
            )
        }
        WorkspaceChangeOpType::DeleteChapter => {
            let (project_id, volume_id, chapter_id) =
                migrate_chapter_target_from_change_set(&rec.changes).ok_or_else(|| {
                    WriterError::Other(format!(
                        "recover_pending_local_delete: cannot unambiguously migrate \
                         delete_target from change_set for journal {} — journal retained",
                        rec.journal_token
                    ))
                })?;
            let project_root = projects_root.join(&project_id);
            let chapter_dir = project_root
                .join("volumes")
                .join(&volume_id)
                .join("chapters")
                .join(&chapter_id);
            if !chapter_dir.exists() {
                return Err(WriterError::Other(format!(
                    "recover_pending_local_delete: old-format journal {} — chapter {} \
                     already absent but no sync_delete_facts to ensure tombstones; \
                     journal retained",
                    rec.journal_token,
                    chapter_dir.display()
                )));
            }
            log::debug!(
                "recover_pending_local_delete: migrated delete_target from change_set for {}",
                chapter_dir.display()
            );
            apply_chapter_delete_or_upgrade(
                app_data_root,
                rec,
                &project_root,
                &volume_id,
                &chapter_id,
                &chapter_dir,
            )
        }
        WorkspaceChangeOpType::DeleteProject => Err(WriterError::Other(format!(
            "recover_pending_local_delete: DeleteProject op_type in generic journal {} \
             — should use project_delete journal; journal retained",
            rec.journal_token
        ))),
    }
}

/// 从旧格式 change_set 中无歧义地提取 Volume delete target。
///
/// 要求 change_set 中所有变更都是 `DeleteTree` 类型，且路径格式为
/// `projects/{pid}/volumes/{vid}`，所有 DeleteTree 指向同一个 `(pid, vid)`。
/// 否则返回 `None`（无法无歧义迁移）。
fn migrate_volume_target_from_change_set(
    change_set: &crate::storage::workspace_git::WorkspaceChangeSet,
) -> Option<(String, String)> {
    use crate::storage::workspace_git::WorkspaceHistoryChange;

    let mut result: Option<(String, String)> = None;
    for change in &change_set.changes {
        let WorkspaceHistoryChange::DeleteTree(path) = change else {
            return None;
        };
        let path_str = path.to_string_lossy();
        let parts: Vec<&str> = path_str.split('/').collect();
        // 路径格式：projects/{pid}/volumes/{vid}
        if parts.len() != 4 || parts[0] != "projects" || parts[2] != "volumes" {
            return None;
        }
        let pid = parts[1].to_string();
        let vid = parts[3].to_string();
        match &result {
            Some((existing_pid, existing_vid)) if existing_pid == &pid && existing_vid == &vid => {}
            Some(_) => return None,
            None => result = Some((pid, vid)),
        }
    }
    result
}

/// 从旧格式 change_set 中无歧义地提取 Chapter delete target。
///
/// 要求 change_set 中所有变更都是 `Delete` 类型，且路径格式为
/// `projects/{pid}/volumes/{vid}/chapters/{cid}/chapter.{meta.json|md}`，
/// 所有 Delete 指向同一个 `(pid, vid, cid)`。
/// 否则返回 `None`（无法无歧义迁移）。
fn migrate_chapter_target_from_change_set(
    change_set: &crate::storage::workspace_git::WorkspaceChangeSet,
) -> Option<(String, String, String)> {
    use crate::storage::workspace_git::WorkspaceHistoryChange;

    let mut result: Option<(String, String, String)> = None;
    for change in &change_set.changes {
        let WorkspaceHistoryChange::Delete(path) = change else {
            return None;
        };
        let path_str = path.to_string_lossy();
        let parts: Vec<&str> = path_str.split('/').collect();
        // 路径格式：projects/{pid}/volumes/{vid}/chapters/{cid}/chapter.{meta.json|md}
        if parts.len() != 7
            || parts[0] != "projects"
            || parts[2] != "volumes"
            || parts[4] != "chapters"
        {
            return None;
        }
        let pid = parts[1].to_string();
        let vid = parts[3].to_string();
        let cid = parts[5].to_string();
        match &result {
            Some((existing_pid, existing_vid, existing_cid))
                if existing_pid == &pid && existing_vid == &vid && existing_cid == &cid => {}
            Some(_) => return None,
            None => result = Some((pid, vid, cid)),
        }
    }
    result
}

/// 根据 sync_delete_facts 幂等补齐 project_root 的 tombstone。
///
/// 恢复 Pending 阶段时，即使源目录已在崩溃前被 move 掉，也要根据 journal 里保存的
/// sync_delete_facts 幂等补齐项目自己的 tombstone，再允许推进到 LocalApplied。
/// 已存在的 tombstone（按 original_path + trash_path 匹配）跳过，保证幂等。
///
/// 对新格式 `DeleteVolume/DeleteChapter`，`facts.is_empty()` 是不可完成的事务——
/// 返回错误，调用方保留 journal，绝不能继续推进 LocalApplied。
fn ensure_tombstones_persisted(
    project_root: &Path,
    facts: &[crate::storage::journal::workspace_change::SyncDeleteFact],
) -> std::result::Result<(), WriterError> {
    if facts.is_empty() {
        // 空 facts 意味着 journal 没有持久化删除事实——这是不可完成的事务。
        // 返回错误，保留 journal，绝不能继续写 history/清 journal。
        return Err(WriterError::Other(
            "ensure_tombstones_persisted: empty sync_delete_facts — \
             cannot complete delete transaction without durable facts; \
             journal retained for manual inspection"
                .to_string(),
        ));
    }
    crate::volume::ensure_tombstones_from_facts(project_root, facts).map_err(WriterError::from)
}

/// 应用打开 workspace 时初始化唯一 Git repo。
///
/// 本地 Git 仓库的生命周期独立于 SyncProvider — 只要 workspace 被打开，
/// Git 历史层就存在，不依赖有没有启用远端同步。
///
/// 返回对应的 `GitRepoLayout`，供调用方注入到 `WriterCoreApi`。
fn ensure_workspace_git(
    app_data_root: &Path,
) -> std::result::Result<crate::storage::git_repo_layout::GitRepoLayout, WriterError> {
    let layout = crate::storage::git_repo_layout::GitRepoLayout::new(app_data_root.to_path_buf());
    crate::storage::workspace_git::ensure_workspace_repo(&layout)?;
    // bootstrap 初始化后实际调用 recover_workspace_crash，
    // 确保打开 workspace 时自动恢复 HEAD/index 损坏。
    match crate::storage::workspace_git::recover_workspace_crash(&layout) {
        Ok(result) => {
            if result.head_was_recovered || result.index_corrupted {
                log::info!(
                    "ensure_workspace_git: recovery performed (head={}, index={})",
                    result.head_was_recovered,
                    result.index_corrupted
                );
            }
        }
        Err(e) => {
            log::warn!(
                "ensure_workspace_git: recover_workspace_crash failed: {}",
                e
            );
        }
    }
    Ok(layout)
}

/// 统一 workspace bootstrap 入口。
///
/// 封装 `ensure_workspace_git` + `recover_storage_transactions`，返回初始化好的
/// `GitRepoLayout`。`open_app_service*` 和 Linux_Qt 的 `create_core_api` 都调用
/// 这一入口，不允许桌面端自己再拼一套初始化。
///
/// - 确保 workspace Git 仓库存在（`.git`）
/// - 恢复 HEAD/index 损坏
/// - 恢复未完成的删除事务
///
/// 返回的 layout 供调用方注入到 `WriterCoreApi` / `WriterAppService`。
pub fn bootstrap_workspace(
    app_data_root: &Path,
) -> std::result::Result<crate::storage::git_repo_layout::GitRepoLayout, WriterError> {
    let layout = ensure_workspace_git(app_data_root)?;
    recover_storage_transactions(app_data_root, &layout)?;
    Ok(layout)
}

/// 统一构造已 bootstrap 的 `WriterCoreApi`。
///
/// 供 Linux_Qt 等需要直接使用 `WriterCoreApi`（而非 `WriterAppService`）的
/// 平台端调用。内部走 `bootstrap_workspace` 确保 `.git` 存在、删除事务已恢复，
/// 并注入正确的 `GitRepoLayout`，不裸构造未 bootstrap 的 API。
pub fn bootstrap_core_api<P1: AsRef<Path>, P2: AsRef<Path>>(
    app_data_root: P1,
    projects_root: P2,
    sync_transport_factory: Option<writer_platform_api::SyncTransportFactory>,
    secure_storage: Option<std::sync::Arc<dyn writer_platform_api::SecureStorage>>,
) -> std::result::Result<WriterCoreApi, WriterError> {
    let app_data_root_path = app_data_root.as_ref();
    let layout = bootstrap_workspace(app_data_root_path)?;
    let api = WriterCoreApi::with_platform_services(
        app_data_root,
        projects_root,
        sync_transport_factory,
        secure_storage,
    );
    api.set_workspace_git_layout(layout);
    Ok(api)
}

/// 用已 bootstrap 的 layout 构造 `WriterCoreApi`，不执行 bootstrap。
///
/// 供平台端普通 getter 使用：用打开 workspace 时保存的 layout 快照构造 API，
/// 不再每次调用都 ensure .git + recover_storage_transactions。
/// 与 [`bootstrap_core_api`] 的区别：本函数不调用 [`bootstrap_workspace`]，
/// 调用方必须传入已 bootstrap 的 layout。
pub fn with_layout_core_api<P1: AsRef<Path>, P2: AsRef<Path>>(
    app_data_root: P1,
    projects_root: P2,
    layout: &crate::storage::git_repo_layout::GitRepoLayout,
    sync_transport_factory: Option<writer_platform_api::SyncTransportFactory>,
    secure_storage: Option<std::sync::Arc<dyn writer_platform_api::SecureStorage>>,
) -> WriterCoreApi {
    let api = WriterCoreApi::with_platform_services(
        app_data_root,
        projects_root,
        sync_transport_factory,
        secure_storage,
    );
    api.set_workspace_git_layout(layout.clone());
    api
}

/// 仅凭根目录打开服务，不注入平台能力。
pub fn open_app_service(
    app_data_root: String,
    projects_root: String,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    crate::storage::git_runtime::ensure_initialized()?;
    // 统一 workspace bootstrap：确保 .git 存在、恢复未完成删除事务。
    let layout = bootstrap_workspace(Path::new(&app_data_root))?;
    let service = Arc::new(WriterAppService::new(app_data_root, projects_root));
    // 注入 bootstrap 计算的 layout 到 API 层。
    service.set_workspace_git_layout(layout);
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service: {e}");
    }
    Ok(service)
}

/// 注入平台初始化信息打开服务。
pub fn open_app_service_with_init(
    app_data_root: String,
    projects_root: String,
    init: PlatformInitDto,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    crate::storage::git_runtime::ensure_initialized()?;
    // 统一 workspace bootstrap：确保 .git 存在、恢复未完成删除事务。
    let layout = bootstrap_workspace(Path::new(&app_data_root))?;
    let platform_init: PlatformInit = init.clone().into();
    let network_state: NetworkState = init.into();

    let services = if let Some(resolver) = get_platform_services_resolver() {
        resolver.resolve(&platform_init, &network_state)
    } else {
        let config_dir = platform_init.app_data_dir.join("config");
        let config_store: Option<Box<dyn ConfigStore>> =
            Some(Box::new(FileConfigStore::new(config_dir)));

        PlatformServices {
            init: platform_init,
            config_store,
            secure_storage: None,
            network_state: Some(network_state),
            sync_transport_factory: None,
        }
    };

    let service = Arc::new(WriterAppService::with_platform_services(
        app_data_root,
        projects_root,
        services,
    ));
    // 注入 bootstrap 计算的 layout 到 API 层。
    service.set_workspace_git_layout(layout);
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service_with_init: {e}");
    }
    Ok(service)
}

/// 注入平台初始化信息与安全存储 callback 打开服务。
#[::uniffi::export]
pub fn open_app_service_with_secure_storage(
    app_data_root: String,
    projects_root: String,
    init: PlatformInitDto,
    secure_storage: Option<Box<dyn SecureStorageProvider>>,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    crate::storage::git_runtime::ensure_initialized()?;
    // 统一 workspace bootstrap：确保 .git 存在、恢复未完成删除事务。
    let layout = bootstrap_workspace(Path::new(&app_data_root))?;
    let platform_init: PlatformInit = init.clone().into();
    let network_state: NetworkState = init.into();

    let secure_storage_impl: Option<Box<dyn SecureStorage>> =
        secure_storage.map(wrap_secure_storage);

    let config_dir = platform_init.app_data_dir.join("config");
    let config_store: Option<Box<dyn ConfigStore>> =
        Some(Box::new(FileConfigStore::new(config_dir)));

    let services = if let Some(resolver) = get_platform_services_resolver() {
        let mut resolved = resolver.resolve(&platform_init, &network_state);
        if secure_storage_impl.is_some() {
            resolved.secure_storage = secure_storage_impl;
        }
        resolved
    } else {
        PlatformServices {
            init: platform_init,
            config_store,
            secure_storage: secure_storage_impl,
            network_state: Some(network_state),
            sync_transport_factory: None,
        }
    };

    let service = Arc::new(WriterAppService::with_platform_services(
        app_data_root,
        projects_root,
        services,
    ));
    // 注入 bootstrap 计算的 layout 到 API 层。
    service.set_workspace_git_layout(layout);
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service_with_secure_storage: {e}");
    }
    Ok(service)
}
