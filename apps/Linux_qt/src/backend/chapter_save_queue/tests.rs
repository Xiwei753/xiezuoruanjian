//! chapter_save_queue 状态机测试（从 mod.rs 拆出）。

use super::*;

fn fake_request_for(
    request_id: u32,
    session: u64,
    snapshot_revision: u64,
    generation: &str,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
) -> ChapterSaveRequest {
    let layout = GitRepoLayout {
        worktree_root: PathBuf::from("/tmp/wt"),
        git_dir: PathBuf::from("/tmp/wt/.git"),
    };
    ChapterSaveRequest {
        identity: ChapterSaveIdentity {
            request_id,
            session_id: session,
            project_id: project_id.to_string(),
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
            snapshot_revision,
            snapshot_sha256: format!("sha-{snapshot_revision}"),
        },
        content: Arc::from(format!("text-{snapshot_revision}")),
        allow_empty_overwrite: false,
        expected: Some(writer_core::api::types::ChapterVersionDto {
            object_generation: generation.to_string(),
            revision: 1,
            content_sha256: "base-sha".to_string(),
        }),
        base_content: Some("base".to_string()),
        context: ChapterSaveContext::new("/tmp", "/tmp/projects", &layout),
    }
}

fn fake_request(
    request_id: u32,
    session: u64,
    snapshot_revision: u64,
    generation: &str,
) -> ChapterSaveRequest {
    fake_request_for(
        request_id,
        session,
        snapshot_revision,
        generation,
        "p1",
        "v1",
        "c1",
    )
}

#[test]
fn higher_revision_supersedes_pending_and_removes_mailbox() {
    let mut store = ChapterSaveStore::default();
    let first = fake_request(1, 7, 5, "g1");
    let second = fake_request(2, 7, 6, "g1");
    let key = save_merge_key(&second);
    register_pending_entry(&mut store, first);
    store.latest_pending.insert(key.clone(), 1);
    register_pending_entry(&mut store, second);
    assert!(supersede_pending(&mut store, 1, 2, 6));
    let entry = store.entries.get(&1).expect("first entry");
    assert_eq!(entry.state, ChapterSaveState::Terminal);
    assert!(matches!(
        entry.outcome,
        ChapterSaveOutcome::Superseded { by: 2 }
    ));
    assert!(entry.content.is_empty());
    assert!(!store.mailbox.contains_key(&1));
    assert!(store.mailbox.contains_key(&2));
    // 接替后旧索引被清掉；enqueue 随后把同一个 key 指向接替者。
    assert_eq!(store.latest_pending.get(&key).copied(), None);
    store.latest_pending.insert(key.clone(), 2);
    assert_eq!(store.latest_pending.get(&key).copied(), Some(2));
}

#[test]
fn started_or_same_revision_is_not_superseded() {
    let mut store = ChapterSaveStore::default();
    register_pending_entry(&mut store, fake_request(10, 3, 4, "g1"));
    // 相同 revision 不允许接替。
    register_pending_entry(&mut store, fake_request(11, 3, 4, "g1"));
    assert!(!supersede_pending(&mut store, 10, 11, 4));
    // 已 Started 的请求也不能被接替。
    register_pending_entry(&mut store, fake_request(12, 3, 5, "g1"));
    if let Some(entry) = store.entries.get_mut(&10) {
        entry.state = ChapterSaveState::Started;
    }
    assert!(!supersede_pending(&mut store, 10, 12, 5));
    assert!(store.mailbox.contains_key(&10));
}

#[test]
fn same_workspace_other_chapter_volume_or_project_does_not_supersede() {
    // 同 workspace + generation + session，但章节/卷/项目不同：合并 key 必须
    // 分开，否则 generation 为空且 session 数值相同时会互相 supersede。
    let first = fake_request_for(20, 9, 5, "g1", "p1", "v1", "c1");
    let other_chapter = fake_request_for(21, 9, 6, "g1", "p1", "v1", "c2");
    let other_volume = fake_request_for(22, 9, 7, "g1", "p1", "v2", "c1");
    let other_project = fake_request_for(23, 9, 8, "g1", "p2", "v1", "c1");

    let first_key = save_merge_key(&first);
    assert_ne!(first_key, save_merge_key(&other_chapter));
    assert_ne!(first_key, save_merge_key(&other_volume));
    assert_ne!(first_key, save_merge_key(&other_project));

    let mut store = ChapterSaveStore::default();
    register_pending_entry(&mut store, first);
    store.latest_pending.insert(first_key, 20);
    // 模拟 enqueue 的合并查找：不同章节/卷/项目都查不到前驱，不接替。
    for request in [other_chapter, other_volume, other_project] {
        let request_id = request.identity.request_id;
        let key = save_merge_key(&request);
        assert_eq!(store.latest_pending.get(&key).copied(), None);
        register_pending_entry(&mut store, request);
        store.latest_pending.insert(key, request_id);
    }
    let first_entry = store.entries.get(&20).expect("first entry");
    assert_eq!(first_entry.state, ChapterSaveState::Pending);
    assert!(matches!(first_entry.outcome, ChapterSaveOutcome::Pending));
    assert!(store.mailbox.contains_key(&20));
}

#[test]
fn prune_rotates_past_undelivered_head_and_reclaims_delivered_entries() {
    let mut store = ChapterSaveStore::default();
    // 最老条目仍是未交付的 Pending：不能因为它而让整张表停止回收。
    register_pending_entry(&mut store, fake_request(1, 1, 1, "g1"));
    for id in 2..=(MAX_RETAINED_RESULTS as u32 + 2) {
        register_pending_entry(&mut store, fake_request(id, 1, id as u64, "g1"));
        // 只验证淘汰规则，outcome 保持 Pending 即可。
        if let Some(entry) = store.entries.get_mut(&id) {
            entry.state = ChapterSaveState::Terminal;
            entry.delivered = true;
        }
    }
    assert!(store.entries.len() < MAX_RETAINED_RESULTS);
    assert!(store.entries.contains_key(&1));
    assert!(store
        .entries
        .contains_key(&(MAX_RETAINED_RESULTS as u32 + 2)));
    assert!(!store.entries.contains_key(&2));
}
