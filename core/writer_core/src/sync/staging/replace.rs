//! #645 评论 5504296097 问题2 修复：ReplaceProject 整树替换 commit plan。
//!
//! `ReplaceProject` 不再走普通 `compute_commit_plan()`（三方合并 + metadata LWW），
//! 而是用本模块的 `build_replace_project_plan` 生成固定语义的 commit plan：
//!
//! - `live ∪ staging` 的路径全集；
//! - staging 有 → `Apply`（用 staging 内容覆盖 live）；
//! - live 有、staging 没有 → `Delete`（live-only 旧文件删除）；
//! - 不要正文 three-way，不要 metadata LWW。
//!
//! 在真正 Apply/Delete 之前，调用方（`apply_staging_commits_for_targets`）用
//! `snapshot_local_records_read_only()` 重新计算当前 local target LWW，与
//! `expected_local_lww` guard 比较：
//! - `current_local == expected_local_lww` → 可以 Replace/Delete；
//! - `current_local > expected_local_lww` / 内容已变化 → 不动 live →
//!   返回 `Err(GuardFailed)`，target 进入 `RecoverableError` / `Retry`。

use std::path::{Path, PathBuf};

use super::commit_plan::{CommitAction, CommitPlan};
use super::run::walk_commit_candidates;

/// #645 评论 5504296097 问题2 修复：ReplaceProject guard 失败错误。
#[derive(Debug)]
pub(crate) enum ReplaceProjectGuardError {
    /// 当前本地 LWW 比 expected 更新（用户在 Transfer 后又编辑了）→ 不动 live。
    LocalAdvanced {
        expected: crate::sync::full_sync::LiveTargetLww,
        current: crate::sync::full_sync::LiveTargetLww,
    },
    /// 重新计算当前本地 LWW 失败（snapshot_local_records_read_only 出错）→ Retry。
    SnapshotFailed(crate::Error),
}

/// #645 评论 5504296097 问题2 修复：ReplaceProject guard 检查结果。
pub(crate) enum ReplaceProjectGuardResult {
    /// Guard 通过，可以执行 replace plan。
    Ok,
    /// Guard 失败，不动 live。
    Err(ReplaceProjectGuardError),
}

/// #645 评论 5504296097 问题2 修复：检查 ReplaceProject guard。
///
/// 用 `snapshot_local_records_read_only()` 重新计算当前 local target LWW，
/// 与 `expected_local_lww` 比较：
/// - `current_local == expected_local_lww` → `Ok`；
/// - `current_local > expected_local_lww` → `Err(LocalAdvanced)`；
/// - snapshot 失败 → `Err(SnapshotFailed)`。
///
/// `expected` 为 `None` 表示 plan 阶段无 live_lww，跳过 guard（兼容旧路径）。
pub(crate) fn check_replace_project_guard(
    live_root: &Path,
    expected: Option<&crate::sync::full_sync::LiveTargetLww>,
) -> ReplaceProjectGuardResult {
    use crate::sync::full_sync::LiveTargetLww;
    use crate::sync::types::SyncScope;

    let Some(expected_lww) = expected else {
        // plan 阶段无 live_lww → 不做 guard 检查（首次 replace 等）。
        return ReplaceProjectGuardResult::Ok;
    };

    let records = match crate::sync::lww::snapshot_local_records_read_only(
        live_root,
        SyncScope::Project,
        &expected_lww.device_id,
    ) {
        Ok(records) => records,
        Err(e) => {
            return ReplaceProjectGuardResult::Err(ReplaceProjectGuardError::SnapshotFailed(e));
        }
    };

    // 从 records 取 max(lww_time, device_id)。
    let winner = records.values().max_by(|a, b| {
        let a_time = lww_record_time_for_manifest_record(a);
        let b_time = lww_record_time_for_manifest_record(b);
        a_time
            .cmp(&b_time)
            .then_with(|| a.device_id.cmp(&b.device_id))
    });

    let current_lww = match winner {
        Some(w) => LiveTargetLww {
            lww_time_ms: lww_record_time_for_manifest_record(w),
            device_id: w.device_id.clone(),
        },
        None => LiveTargetLww {
            lww_time_ms: 0,
            device_id: expected_lww.device_id.clone(),
        },
    };

    // current_local == expected → Ok；current_local > expected → Err。
    let equal = current_lww.lww_time_ms == expected_lww.lww_time_ms
        && current_lww.device_id == expected_lww.device_id;
    if equal {
        return ReplaceProjectGuardResult::Ok;
    }

    // current > expected？（时间更大，或时间相同 device_id 更大）
    let current_advances = current_lww.lww_time_ms > expected_lww.lww_time_ms
        || (current_lww.lww_time_ms == expected_lww.lww_time_ms
            && current_lww.device_id > expected_lww.device_id);
    if current_advances {
        return ReplaceProjectGuardResult::Err(ReplaceProjectGuardError::LocalAdvanced {
            expected: expected_lww.clone(),
            current: current_lww,
        });
    }

    // current < expected（plan 阶段的 lww 比现在还新？不应发生，但保守放行）。
    log::warn!(
        "[sync] check_replace_project_guard: current lww ({:?}) older than expected ({:?}) \
         — allowing replace (unexpected but safe)",
        current_lww,
        expected_lww
    );
    ReplaceProjectGuardResult::Ok
}

/// #645 评论 5504296097 问题2 修复：计算 manifest record 的 LWW 时间。
fn lww_record_time_for_manifest_record(r: &crate::sync::types::ManifestFileRecord) -> i64 {
    if r.op == "delete" {
        r.deleted_at_ms.unwrap_or(r.updated_at_ms)
    } else {
        r.updated_at_ms
    }
}

/// #645 评论 5504296097 问题2 修复：构建 ReplaceProject 整树替换 commit plan。
///
/// 语义固定为：
/// - `live ∪ staging` 的路径全集；
/// - staging 有 → `Apply`（用 staging 内容覆盖 live）；
/// - live 有、staging 没有 → `Delete`（live-only 旧文件删除）；
/// - 不要正文 three-way，不要 metadata LWW。
///
/// `engine_state_actions` 收集 staging 里的 sync engine state
/// （manifest.sync.json / state.local.json / conflicts.json），直接写回 live。
pub(crate) fn build_replace_project_plan(
    live_root: &Path,
    staging_root: &Path,
) -> crate::error::Result<CommitPlan> {
    let mut plan = CommitPlan::default();

    let staging_paths = list_commit_candidate_paths(staging_root)?;
    let live_paths = list_commit_candidate_paths(live_root)?;

    let mut all_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for p in &staging_paths {
        all_paths.insert(p.clone());
    }
    for p in &live_paths {
        all_paths.insert(p.clone());
    }

    for rel in all_paths {
        let rel_str = rel.to_string_lossy().to_string();

        // 按 StagingCommitClass 决定写回语义（与 compute_commit_plan 一致）。
        match classify_staging_commit_path(&rel_str) {
            StagingCommitClass::Skip => {
                // .git/、full-sync-staging/、app-meta/transactions/、
                // config.local.json、secrets：永不进 commit。
                continue;
            }
            StagingCommitClass::EngineState => {
                // app-meta/sync/manifest.sync.json、state.local.json、conflicts.json：
                // Transfer 在 staging 里更新了它们，Commit 直接写回 live。
                let staging_path = staging_root.join(&rel);
                let incoming: Option<Vec<u8>> = if staging_path.exists() {
                    Some(read_bytes(&staging_path)?)
                } else {
                    None
                };
                apply_incoming(&mut plan, rel, incoming, StagingCommitClass::EngineState);
                continue;
            }
            StagingCommitClass::Content => {
                // 走下方整树替换语义。
            }
        }

        let staging_path = staging_root.join(&rel);
        let live_path = live_root.join(&rel);

        if staging_path.exists() {
            // staging 有 → Apply（用 staging 内容覆盖 live）。
            let content = read_bytes(&staging_path)?;
            plan.content_actions.push(CommitAction::Apply {
                rel_path: rel,
                content,
            });
        } else if live_path.exists() {
            // live 有、staging 没有 → Delete。
            plan.content_actions
                .push(CommitAction::Delete { rel_path: rel });
        }
        // 两者都不存在 → 不可能（all_paths 来自 live ∪ staging）。
    }

    Ok(plan)
}

/// 列出 root 下参与 commit 比较的候选路径（与 staging/run.rs 共用同一套跳过规则）。
fn list_commit_candidate_paths(root: &Path) -> crate::error::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    walk_commit_candidates(root, root, &mut out)?;
    Ok(out)
}

fn read_bytes(path: &Path) -> crate::error::Result<Vec<u8>> {
    Ok(std::fs::read(path)?)
}

// 复用 staging/run.rs 和 commit_plan.rs 的分类逻辑。
use super::commit_plan::{apply_incoming, classify_staging_commit_path, StagingCommitClass};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(root: &Path, rel: &str, content: &[u8]) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }

    #[test]
    fn replace_plan_staging_overrides_live() {
        let live = TempDir::new().unwrap();
        let staging = TempDir::new().unwrap();
        // live 有 old.txt，staging 有 new.txt + shared.txt
        write_file(live.path(), "old.txt", b"old");
        write_file(live.path(), "shared.txt", b"live-shared");
        write_file(staging.path(), "new.txt", b"new");
        write_file(staging.path(), "shared.txt", b"staging-shared");

        let plan = build_replace_project_plan(live.path(), staging.path()).unwrap();
        // staging 有 → Apply
        let applied: std::collections::HashMap<String, Vec<u8>> = plan
            .content_actions
            .iter()
            .filter_map(|a| match a {
                CommitAction::Apply { rel_path, content } => {
                    Some((rel_path.to_string_lossy().to_string(), content.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            applied.get("new.txt").map(|c| c.as_slice()),
            Some(b"new".as_slice())
        );
        assert_eq!(
            applied.get("shared.txt").map(|c| c.as_slice()),
            Some(b"staging-shared".as_slice())
        );
        // live 有、staging 没有 → Delete
        let deleted: std::collections::HashSet<String> = plan
            .content_actions
            .iter()
            .filter_map(|a| match a {
                CommitAction::Delete { rel_path } => Some(rel_path.to_string_lossy().to_string()),
                _ => None,
            })
            .collect();
        assert!(deleted.contains("old.txt"));
    }

    #[test]
    fn replace_guard_equal_passes() {
        let live = TempDir::new().unwrap();
        // 空 live → current lww = (0, device_id)，与 expected (0, device_id) 相等。
        let expected = crate::sync::full_sync::LiveTargetLww {
            lww_time_ms: 0,
            device_id: "dev-1".to_string(),
        };
        let result = check_replace_project_guard(live.path(), Some(&expected));
        assert!(matches!(result, ReplaceProjectGuardResult::Ok));
    }

    #[test]
    fn replace_guard_none_expected_passes() {
        let live = TempDir::new().unwrap();
        let result = check_replace_project_guard(live.path(), None);
        assert!(matches!(result, ReplaceProjectGuardResult::Ok));
    }
}
