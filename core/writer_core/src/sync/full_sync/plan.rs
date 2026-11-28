//! Full sync plan builder — target enumeration only.
//!
//! Target 只由两类事实构成：
//! - 本机事实：存在的作品目录、App 数据组、待删除作品的 tombstone；
//! - 远端事实：远端 `projects/<id>/` 下仍有普通对象。
//!
//! 删除、恢复都不再是独立枚举的 LWW 决策，而是对象比较的结果：
//! - 本机已删除作品 → `DeleteRemoteProject`，由对象引擎把远端对象逐个删除；
//! - 远端独有作品 → `RestoreProject`，由对象引擎把远端对象逐个下载并提交；
//! - 两边都有 → `LiveProject`，按对象版本比较。

use std::path::Path;

use crate::sync::types::PlannedTargetKind;

use super::PlannedTarget;

/// 无副作用共享 target planner。
///
/// 正式同步与 dry run 都调用本函数，不再各自维护一套 target 枚举。
/// `staging_root` 全部为 `None`，由 `prepare_staging_runs` 填充。
pub fn build_full_sync_target_plan(
    app_data_root: &Path,
    projects_root: &Path,
    live_projects: &[crate::project::Project],
    remote_project_ids: &[String],
    pending_deleted: &[crate::sync::types::PendingDeletedTarget],
) -> crate::Result<Vec<PlannedTarget>> {
    let mut targets = Vec::new();

    // App 数据组：本机发现的组 + 有星图时补索引组。
    let mut app_groups = std::collections::BTreeMap::new();
    for group in crate::sync::app_data_group_catalog::discover_local_app_data_groups(app_data_root)?
    {
        app_groups.insert(group.id(), group);
    }
    if app_groups
        .values()
        .any(|group| matches!(group, crate::sync::types::AppDataGroup::Starmap(_)))
    {
        let index_group = crate::sync::types::AppDataGroup::StarmapIndex;
        app_groups.insert(index_group.id(), index_group);
    }
    let mut app_groups: Vec<_> = app_groups.into_values().collect();
    app_groups.sort_by_key(|group| match group {
        crate::sync::types::AppDataGroup::Global => (0_u8, group.id()),
        crate::sync::types::AppDataGroup::Starmap(_) => (1, group.id()),
        crate::sync::types::AppDataGroup::StarmapIndex => (2, group.id()),
        crate::sync::types::AppDataGroup::StatsDevice(_) => (3, group.id()),
    });
    for group in app_groups {
        targets.push(PlannedTarget {
            target: crate::sync::types::SyncTarget::app_data_group(group),
            local_root: app_data_root.to_path_buf(),
            staging_root: None,
            target_kind: PlannedTargetKind::App,
            project_id: None,
            target_live_root: app_data_root.to_path_buf(),
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: None,
            expected_delete_lww: None,
        });
    }

    let mut planned_ids = std::collections::BTreeSet::new();
    for pending in pending_deleted {
        // target_id 必须严格解析成作品 id；非法记录跳过而不是猜。
        let project_id = match project_id_from_target_id(&pending.target.remote_prefix) {
            Ok(id) => id,
            Err(error) => {
                log::warn!(
                    "[sync] plan: skip pending deleted target {:?}: {error}",
                    pending.target.remote_prefix
                );
                continue;
            }
        };
        if !planned_ids.insert(project_id.clone()) {
            continue;
        }
        let project_root = projects_root.join(&project_id);
        targets.push(PlannedTarget {
            target: pending.target.clone(),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::DeleteRemoteProject,
            project_id: Some(project_id),
            target_live_root: project_root,
            deleted_journal_token: Some(pending.journal_token.clone()),
            deleted_lww: None,
            live_lww: None,
            expected_delete_lww: None,
        });
    }

    for project in live_projects {
        if !planned_ids.insert(project.id.clone()) {
            continue;
        }
        let project_root = projects_root.join(&project.id);
        targets.push(PlannedTarget {
            target: crate::sync::types::SyncTarget::project(&project.id),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some(project.id.clone()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: None,
            expected_delete_lww: None,
        });
    }

    for project_id in remote_project_ids {
        if !planned_ids.insert(project_id.clone()) {
            continue;
        }
        let project_root = projects_root.join(project_id);
        targets.push(PlannedTarget {
            target: crate::sync::types::SyncTarget::project(project_id),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::RestoreProject,
            project_id: Some(project_id.clone()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: None,
            expected_delete_lww: None,
        });
    }

    Ok(targets)
}

/// `projects/<id>` → `<id>`；只接受单个安全 path segment。
fn project_id_from_target_id(target_id: &str) -> Result<String, String> {
    let raw = target_id
        .strip_prefix("projects/")
        .ok_or_else(|| format!("target id '{target_id}' is not under projects/"))?;
    let project_id = raw.trim_end_matches('/');
    if project_id.is_empty()
        || project_id == "."
        || project_id == ".."
        || project_id.contains('/')
        || project_id.contains('\\')
    {
        return Err(format!("invalid project id '{project_id}'"));
    }
    Ok(project_id.to_string())
}
