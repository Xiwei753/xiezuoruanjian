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

mod restore_draft_tests {
    use super::super::restore_draft::{
        base_matches_plan, baseline_text_from_projection, draft_dir_is_empty, original_base_text,
        restore_commit_revision, unchanged_baseline, version_display,
    };
    use crate::backend::chapter_draft::{ChapterDraftRestorePlan, LoadedFact};
    use writer_core::api::types::ChapterVersionDto;

    fn plan(fact_count: usize, text_revision: u64, event_end: u64) -> ChapterDraftRestorePlan {
        ChapterDraftRestorePlan {
            volume_id: "v1".to_string(),
            chapter_id: "c1".to_string(),
            base_generation: "g1".to_string(),
            base_revision: 1,
            base_sha256: "sha".to_string(),
            text: None,
            text_revision,
            text_sha256: String::new(),
            fact_count,
            batch_json: "[]".to_string(),
            facts: if fact_count > 0 {
                vec![LoadedFact {
                    event_id: "e1".to_string(),
                    revision: event_end,
                    projection_json: "{}".to_string(),
                }]
            } else {
                Vec::new()
            },
            event_end_revision: event_end,
            session_id: 7,
            generation: "g1".to_string(),
            mixed_sessions: false,
            editor_base_revision: 0,
            editor_base_sha256: String::new(),
        }
    }

    #[test]
    fn empty_draft_cleanup_requires_no_pending_facts_or_text() {
        assert!(draft_dir_is_empty(0, 0, false));
        // 首次事件只有 spool、facts/text 尚未写出：绝不能按旧摘要删除。
        assert!(!draft_dir_is_empty(1, 0, false));
        assert!(!draft_dir_is_empty(0, 1, false));
        assert!(!draft_dir_is_empty(0, 0, true));
    }

    #[test]
    fn replay_terminal_text_commits_at_event_end_revision() {
        // text.txt 落后一个事件（revision 4），事件链到 5：提交必须用 5。
        assert_eq!(restore_commit_revision(&plan(2, 4, 5)), 5);
        // 没有事实时仍用正文 revision。
        assert_eq!(restore_commit_revision(&plan(0, 4, 0)), 4);
    }

    #[test]
    #[test]
    fn empty_baseline_is_a_real_baseline() {
        // Issue #1062 评论 7192111899 第 3 项：空章的基线是合法空串。
        assert_eq!(
            baseline_text_from_projection(r#"{"baselineText":""}"#),
            Some(String::new())
        );
        assert_eq!(
            baseline_text_from_projection(r#"{"baselineText":"abc"}"#),
            Some("abc".to_string())
        );
        assert_eq!(baseline_text_from_projection(r#"{}"#), None);
    }

    #[test]
    fn unchanged_baseline_requires_same_editor_revision_and_digest() {
        let mut plan = plan(0, 3, 0);
        // 章节已保存过 30 次、编辑器从 0 重新打开：持久 revision 不参与比较。
        plan.base_revision = 30;
        assert!(
            !unchanged_baseline(&plan),
            "persistent base revision must not verify an edited editor snapshot"
        );
        plan.base_revision = 1;
        plan.text = Some("abc".to_string());
        plan.text_revision = 3;
        plan.editor_base_revision = 3;
        plan.editor_base_sha256 = "digest-abc".to_string();
        plan.text_sha256 = "digest-abc".to_string();
        assert!(unchanged_baseline(&plan));

        plan.text_revision = 4;
        assert!(!unchanged_baseline(&plan), "编辑过但没有事实不能当基线");
        plan.text_revision = 3;
        plan.text_sha256 = "digest-other".to_string();
        assert!(!unchanged_baseline(&plan), "摘要不一致不能当基线");
        plan.editor_base_sha256.clear();
        assert!(!unchanged_baseline(&plan), "没有基线摘要不能证明未修改");
    }

    #[test]
    fn original_base_text_accepts_empty_chapter_baseline() {
        let mut plan = plan(1, 0, 1);
        plan.facts = vec![LoadedFact {
            event_id: "e1".to_string(),
            revision: 1,
            projection_json: r#"{"event":{},"baselineText":""}"#.to_string(),
        }];
        assert_eq!(
            original_base_text(&plan),
            Some(String::new()),
            "新建作品的第一个空章必须能还原出空基线"
        );
    }

    #[test]
    fn baseline_text_comes_from_first_fact_projection() {
        // Core 的 ChapterProofProjection 以 camelCase 序列化。
        assert_eq!(
            baseline_text_from_projection(r#"{"event":{},"baselineText":"abc"}"#),
            Some("abc".to_string())
        );
        // 缺失/null 表示没有可恢复的旧基线；空串是合法空章基线。
        assert_eq!(baseline_text_from_projection(r#"{"event":{}}"#), None);
        assert_eq!(
            baseline_text_from_projection(r#"{"baselineText":null}"#),
            None
        );
        assert_eq!(
            baseline_text_from_projection(r#"{"baselineText":""}"#),
            Some(String::new())
        );
        // JSON 损坏不能猜测基线。
        assert_eq!(baseline_text_from_projection("not json"), None);

        // original_base_text 只取事件链第一条事实的基线。
        let mut draft = plan(1, 0, 5);
        draft.facts[0].projection_json = r#"{"baselineText":"abc"}"#.to_string();
        draft.facts.push(LoadedFact {
            event_id: "e2".to_string(),
            revision: 6,
            projection_json: r#"{"baselineText":"later"}"#.to_string(),
        });
        assert_eq!(original_base_text(&draft), Some("abc".to_string()));
        // 没有事实或第一条事实没有基线时不可恢复。
        assert_eq!(original_base_text(&plan(0, 0, 0)), None);
        let mut missing = plan(1, 0, 5);
        missing.facts[0].projection_json = r#"{"event":{}}"#.to_string();
        assert_eq!(original_base_text(&missing), None);
    }

    #[test]
    fn base_matches_requires_generation_revision_and_sha() {
        let draft = plan(0, 0, 0);
        let version = ChapterVersionDto {
            object_generation: "g1".to_string(),
            revision: 1,
            content_sha256: "sha".to_string(),
        };
        assert!(base_matches_plan(&draft, &version));
        // revision / generation / sha 任一不同都不算同一条基线。
        assert!(!base_matches_plan(
            &draft,
            &ChapterVersionDto {
                object_generation: "g1".to_string(),
                revision: 2,
                content_sha256: "sha".to_string(),
            }
        ));
        assert!(!base_matches_plan(
            &draft,
            &ChapterVersionDto {
                object_generation: "g2".to_string(),
                revision: 1,
                content_sha256: "sha".to_string(),
            }
        ));
        assert!(!base_matches_plan(
            &draft,
            &ChapterVersionDto {
                object_generation: "g1".to_string(),
                revision: 1,
                content_sha256: "other".to_string(),
            }
        ));
        // base 为空（generation 空、revision 0）不可校验，即使磁盘版本全空也不匹配。
        let mut unknown = plan(0, 0, 0);
        unknown.base_generation = String::new();
        unknown.base_revision = 0;
        assert!(!base_matches_plan(&unknown, &version));
        assert!(!base_matches_plan(&unknown, &ChapterVersionDto::default()));
    }
}
