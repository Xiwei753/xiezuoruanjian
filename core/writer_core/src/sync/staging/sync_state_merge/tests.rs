//! sync_state_merge.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;
use crate::sync::types::{SyncConflict, SyncConflictKind, SyncState};
use tempfile::TempDir;

/// 写入 SyncState 到 `root/app-meta/sync/state.local.json`。
fn write_state(root: &Path, state: &SyncState) {
    let dir = root.join("app-meta/sync");
    std::fs::create_dir_all(&dir).unwrap();
    let json = serde_json::to_string_pretty(state).unwrap();
    std::fs::write(dir.join("state.local.json"), json).unwrap();
}

/// 写入 conflicts 到 `root/app-meta/sync/conflicts.json`。
fn write_conflicts(root: &Path, conflicts: &[SyncConflict]) {
    let dir = root.join("app-meta/sync");
    std::fs::create_dir_all(&dir).unwrap();
    let json = serde_json::to_string_pretty(conflicts).unwrap();
    std::fs::write(dir.join("conflicts.json"), json).unwrap();
}

fn make_conflict(path: &str) -> SyncConflict {
    SyncConflict {
        local_path: path.to_string(),
        remote_path: path.to_string(),
        kind: SyncConflictKind::BothChanged,
        local_hash: "local".to_string(),
        remote_hash: "remote".to_string(),
        base_hash: "base".to_string(),
        created_at: 1,
        description: "test".to_string(),
        remote_snapshot_path: None,
    }
}

fn make_state(conflicted: &[&str], known: &[(&str, &str)]) -> SyncState {
    let mut state = SyncState::default();
    for p in conflicted {
        state.conflicted_files.insert(p.to_string());
    }
    for (p, h) in known {
        state.known_files.insert(p.to_string(), h.to_string());
    }
    state
}

/// base 有冲突 A，live 已解决（无 A），staging 仍有 A → 不复活。
#[test]
fn user_resolved_conflict_is_not_revived() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    let conflict_a = make_conflict("a.md");
    write_state(&base_root, &make_state(&["a.md"], &[("a.md", "base-hash")]));
    write_conflicts(&base_root, std::slice::from_ref(&conflict_a));

    // live: 用户已解决 A，known_files 更新为 remote_hash
    write_state(&live_root, &make_state(&[], &[("a.md", "remote-hash")]));
    write_conflicts(&live_root, &[]);

    // staging: 仍有旧冲突 A（seed 时复制）
    write_state(
        &staging_root,
        &make_state(&["a.md"], &[("a.md", "base-hash")]),
    );
    write_conflicts(&staging_root, std::slice::from_ref(&conflict_a));

    let (merged_state, merged_conflicts) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert!(
        !merged_state.conflicted_files.contains("a.md"),
        "用户已解决的冲突不应复活"
    );
    assert!(
        merged_conflicts.iter().all(|c| c.local_path != "a.md"),
        "conflicts.json 中不应有已解决的冲突"
    );
    assert_eq!(
        merged_state.known_files.get("a.md").map(String::as_str),
        Some("remote-hash"),
        "known_files 应保留 live 的解决结果，不被 staging 旧值覆盖"
    );
}

/// base 无冲突，staging 新增冲突 B → 写入 live。
#[test]
fn new_conflict_from_staging_is_written() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    let conflict_b = make_conflict("b.md");
    write_state(&base_root, &SyncState::default());
    write_conflicts(&base_root, &[]);

    write_state(&live_root, &SyncState::default());
    write_conflicts(&live_root, &[]);

    write_state(
        &staging_root,
        &make_state(&["b.md"], &[("b.md", "staging-hash")]),
    );
    write_conflicts(&staging_root, std::slice::from_ref(&conflict_b));

    let (merged_state, merged_conflicts) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert!(
        merged_state.conflicted_files.contains("b.md"),
        "staging 新增冲突应写入 live"
    );
    assert!(
        merged_conflicts.iter().any(|c| c.local_path == "b.md"),
        "conflicts.json 应包含新冲突"
    );
}

/// base/live/staging 都有冲突 → 保留 unresolved。
#[test]
fn unresolved_conflict_is_preserved() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    let conflict_c = make_conflict("c.md");
    write_state(&base_root, &make_state(&["c.md"], &[("c.md", "base-hash")]));
    write_conflicts(&base_root, std::slice::from_ref(&conflict_c));

    write_state(&live_root, &make_state(&["c.md"], &[("c.md", "base-hash")]));
    write_conflicts(&live_root, std::slice::from_ref(&conflict_c));

    write_state(
        &staging_root,
        &make_state(&["c.md"], &[("c.md", "base-hash")]),
    );
    write_conflicts(&staging_root, std::slice::from_ref(&conflict_c));

    let (merged_state, merged_conflicts) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert!(
        merged_state.conflicted_files.contains("c.md"),
        "未解决的冲突应保留"
    );
    assert!(
        merged_conflicts.iter().any(|c| c.local_path == "c.md"),
        "conflicts.json 应保留未解决冲突"
    );
}

/// staging 不存在 → 返回 live 状态。
#[test]
fn staging_missing_returns_live() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    write_state(&base_root, &SyncState::default());
    write_conflicts(&base_root, &[]);

    let live_state = make_state(&["x.md"], &[("x.md", "live-hash")]);
    write_state(&live_root, &live_state);
    write_conflicts(&live_root, &[]);

    // staging 目录存在但无 state.local.json
    std::fs::create_dir_all(staging_root.join("app-meta/sync")).unwrap();

    let (merged_state, merged_conflicts) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert!(
        merged_state.conflicted_files.contains("x.md"),
        "staging 不存在时应返回 live 状态"
    );
    assert!(merged_conflicts.is_empty());
}

/// pending_take_remote 保留 live 值，不被 staging 覆盖。
#[test]
fn pending_take_remote_preserves_live() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    write_state(&base_root, &SyncState::default());
    write_conflicts(&base_root, &[]);

    let mut live_state = SyncState::default();
    live_state
        .pending_take_remote
        .insert("pending.md".to_string());
    write_state(&live_root, &live_state);
    write_conflicts(&live_root, &[]);

    let staging_state = SyncState::default();
    // staging 不应有 pending_take_remote
    write_state(&staging_root, &staging_state);
    write_conflicts(&staging_root, &[]);

    let (merged_state, _) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert!(
        merged_state.pending_take_remote.contains("pending.md"),
        "pending_take_remote 应保留 live 值"
    );
}

/// device_id 保留 live 值，不被 staging 覆盖。
#[test]
fn device_id_preserves_live() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    write_state(&base_root, &SyncState::default());
    write_conflicts(&base_root, &[]);

    let mut live_state = SyncState::default();
    live_state.device_id = "live-device".to_string();
    write_state(&live_root, &live_state);
    write_conflicts(&live_root, &[]);

    let mut staging_state = SyncState::default();
    staging_state.device_id = "staging-device".to_string();
    write_state(&staging_root, &staging_state);
    write_conflicts(&staging_root, &[]);

    let (merged_state, _) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert_eq!(
        merged_state.device_id, "live-device",
        "device_id 应保留 live 值"
    );
}

/// 真实首次同步：live/base 无 state（device_id 空），staging Transfer 生成稳定
/// device_id → 合并结果必须使用 staging 的 device_id，不能因为 live 优先而丢掉。
///
/// 生产顺序：live 还没有 state.local.json → seed 后 base/live 都没 state →
/// Transfer 在 staging 生成 state.local.json（#761 保证用平台稳定 device_id）→
/// Commit 三方 merge。修复前合并器无条件选 live（default 的随机 UUID）把 staging
/// 的稳定 device_id 丢掉，把 #761 的首次同步 device identity 修复打回去。
#[test]
fn device_id_first_sync_uses_staging_when_live_empty() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    // base/live 都没有 state.local.json（首次同步）
    // read_sync_state_or_default 返回 default（device_id 是随机 UUID，非空但无意义）
    std::fs::create_dir_all(base_root.join("app-meta/sync")).unwrap();
    std::fs::create_dir_all(live_root.join("app-meta/sync")).unwrap();

    // staging: Transfer 生成的稳定 device_id
    let mut staging_state = SyncState::default();
    staging_state.device_id = "stable-device-from-transfer".to_string();
    write_state(&staging_root, &staging_state);
    write_conflicts(&staging_root, &[]);

    let (merged_state, _) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert_eq!(
        merged_state.device_id, "stable-device-from-transfer",
        "首次同步时 live 无 state，合并结果必须使用 staging 生成的稳定 device_id"
    );
}

/// live 有 state.local.json 但 device_id 为空（合法旧数据/迁移输入），
/// staging 已被 Transfer 修成稳定 device_id → 合并结果必须用 staging 的稳定值，
/// 不能因为 live state 文件"存在"就选 live 的空 device_id。
///
/// 生产顺序：live 已有旧 state.local.json（device_id=""）；seed 复制到 base/staging；
/// Transfer 在 staging 用 preferred device_id 修成稳定值；live 仍为空；
/// Commit 进入 merge。修复前因 live state 文件存在直接选 live_state.device_id=""，
/// 把 staging 修好的稳定 device_id 覆盖成空。
#[test]
fn device_id_uses_staging_when_live_file_exists_but_field_empty() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    // base: 旧数据，state.local.json 存在但 device_id 为空
    let mut base_state = SyncState::default();
    base_state.device_id = "".to_string();
    write_state(&base_root, &base_state);
    write_conflicts(&base_root, &[]);

    // live: 旧数据，state.local.json 存在但 device_id 为空
    let mut live_state = SyncState::default();
    live_state.device_id = "".to_string();
    write_state(&live_root, &live_state);
    write_conflicts(&live_root, &[]);

    // staging: Transfer 已用 preferred device_id 修成稳定值
    let mut staging_state = SyncState::default();
    staging_state.device_id = "stable-device".to_string();
    write_state(&staging_root, &staging_state);
    write_conflicts(&staging_root, &[]);

    // 确认 base/live 的 state.local.json 都真实存在（测试前提）
    assert!(
        base_root.join("app-meta/sync/state.local.json").exists(),
        "测试前提：base 应有 state.local.json"
    );
    assert!(
        live_root.join("app-meta/sync/state.local.json").exists(),
        "测试前提：live 应有 state.local.json"
    );

    let (merged_state, _) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert_eq!(
        merged_state.device_id, "stable-device",
        "live state 文件存在但 device_id 为空时，合并结果必须使用 staging 的稳定 device_id，\
         不能因为文件存在就选 live 的空值"
    );
}

/// Transfer 成功消费 pending 后不能复活：base={A}, live={A}, staging={} → merged 必须空。
///
/// 生产顺序：同步前 live 已有旧兼容数据 pending_take_remote={A}；seed 后 base/staging
/// 也有 A；Transfer 在 staging 成功下载 A 并按 LWW 把 A 从 staging pending 移除；
/// live 没有并发用户改动仍为 {A}。修复前合并器无条件复制 live={A}，A 被重新写回
/// pending，下一轮重复执行 take_remote。
#[test]
fn pending_take_remote_consumed_by_transfer_not_revived() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    // base = {A}
    let mut base_state = SyncState::default();
    base_state.pending_take_remote.insert("a.md".to_string());
    write_state(&base_root, &base_state);
    write_conflicts(&base_root, &[]);

    // live = {A}（用户没并发改动，与 base 一致）
    let mut live_state = SyncState::default();
    live_state.pending_take_remote.insert("a.md".to_string());
    write_state(&live_root, &live_state);
    write_conflicts(&live_root, &[]);

    // staging = {}（Transfer 成功消费 A，从 pending 移除）
    write_state(&staging_root, &SyncState::default());
    write_conflicts(&staging_root, &[]);

    let (merged_state, _) =
        merge_sync_state_three_way(&base_root, &live_root, &staging_root).unwrap();

    assert!(
        merged_state.pending_take_remote.is_empty(),
        "Transfer 成功消费的 pending 不应被 live 无条件复制复活"
    );
}

/// Issue #762 评论 5831990584 问题 1 复现：
/// live 的 state.local.json 真实存在但写入非法 JSON 时，
/// `read_sync_state_or_default()` 会通过 `unwrap_or_default()` 回退到
/// `SyncState::default()`，其 device_id 是随机 UUID（非空但无意义）。
/// `merge_device_id_three_way()` 看到 live 文件存在且 device_id 非空，
/// 会把这个随机 UUID 当成"有效 live device_id"返回，导致
/// `merge_sync_state_three_way()` 错误地返回 Ok（伪装的随机设备状态）。
///
/// 正式加载逻辑 `config_store.rs::load_sync_state_with_preferred_device_id`
/// 对"文件存在但解析失败"明确返回 Err。合并阶段也应返回 Err，而不是
/// 静默把损坏文件伪装成一份新的随机设备状态参与三方判断。
///
/// 修复前：本测试在 `assert!(result.is_err())` 处失败——
/// `merge_sync_state_three_way` 返回 Ok，device_id 是随机 UUID。
#[test]
fn corrupt_live_state_json_must_return_err_not_fabricate_random_device_id() {
    let tmp = TempDir::new().unwrap();
    let base_root = tmp.path().join("base");
    let live_root = tmp.path().join("live");
    let staging_root = tmp.path().join("staging");

    // base: 合法的 state（device_id 稳定）
    let mut base_state = SyncState::default();
    base_state.device_id = "base-device".to_string();
    write_state(&base_root, &base_state);
    write_conflicts(&base_root, &[]);

    // live: state.local.json 真实存在但内容是非法 JSON
    let live_sync_dir = live_root.join("app-meta/sync");
    std::fs::create_dir_all(&live_sync_dir).unwrap();
    std::fs::write(live_sync_dir.join("state.local.json"), "{not valid json").unwrap();
    // 确认测试前提：live 的 state.local.json 真实存在
    assert!(
        live_root.join("app-meta/sync/state.local.json").exists(),
        "测试前提：live 应有损坏的 state.local.json"
    );

    // staging: 合法的 state（device_id 稳定），避免 staging 不存在时 early return
    let mut staging_state = SyncState::default();
    staging_state.device_id = "staging-device".to_string();
    write_state(&staging_root, &staging_state);
    write_conflicts(&staging_root, &[]);

    let result = merge_sync_state_three_way(&base_root, &live_root, &staging_root);

    // 期望：live 的 state.local.json 存在但解析失败 → 返回 Err
    // 当前（未修复）代码：返回 Ok，device_id 是随机 UUID → 此断言失败
    assert!(
        result.is_err(),
        "live 的 state.local.json 存在但 JSON 损坏时，merge_sync_state_three_way 必须返回 Err，\
         不能把损坏文件伪装成一份随机 device_id 的默认状态参与三方合并。\
         实际得到：{:?}",
        result.as_ref().err().map(|e| e.to_string())
    );

    // 若修复后返回 Err，进一步验证错误信息提及解析失败（可选，不强制措辞）。
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            msg.contains("parse") || msg.contains("json") || msg.contains("state.local.json"),
            "错误信息应提及 state.local.json 解析失败，实际：{msg}"
        );
    }
}
