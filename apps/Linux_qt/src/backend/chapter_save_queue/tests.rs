//! chapter_save_queue 状态机测试（从 mod.rs 拆出）。

use super::*;

fn fake_request(
    request_id: u32,
    session: u64,
    snapshot_revision: u64,
    generation: &str,
) -> ChapterSaveRequest {
    let layout = GitRepoLayout {
        worktree_root: PathBuf::from("/tmp/wt"),
        git_dir: PathBuf::from("/tmp/wt/.git"),
    };
    ChapterSaveRequest {
        identity: ChapterSaveIdentity {
            request_id,
            session_id: session,
            project_id: "p1".to_string(),
            volume_id: "v1".to_string(),
            chapter_id: "c1".to_string(),
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
