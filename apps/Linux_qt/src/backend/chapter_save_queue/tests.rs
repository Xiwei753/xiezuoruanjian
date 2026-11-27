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
            committed_text_revision: 0,
            committed_text_sha256: String::new(),
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

mod restore_contract_tests {
    use super::super::draft_jobs::draft_restore_status;
    use super::super::restore_draft::handle_restore_project;
    use super::super::worker::ChapterSaveWorkerState;
    use super::super::{ChapterRestoreCommand, ChapterSaveContext};
    use crate::backend::chapter_draft;
    use std::path::PathBuf;
    use writer_core::api::WriterCoreApi;
    use writer_core::writing_stats::proof::content_digest_of_text;

    fn workspace() -> (tempfile::TempDir, WriterCoreApi, String, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let projects_root = temp.path().join("projects");
        let api = WriterCoreApi::new(temp.path(), projects_root.clone());
        let project = api.create_project("作品").unwrap();
        let volumes = api.list_volumes(&project.id).unwrap();
        let volume_id = volumes.first().unwrap().id.clone();
        let chapter = api
            .create_chapter(&project.id, &volume_id, "第一章")
            .unwrap();
        // 推进到 revision 30，复现“章节已保存多次、编辑器重新从 0/1 打开”。
        for revision in 0..30 {
            api.save_chapter_content_versioned(
                &project.id,
                &volume_id,
                &chapter.id,
                &format!("已提交正文 {}", revision + 1),
                None,
                None,
            )
            .unwrap();
        }
        (temp, api, project.id, volume_id, chapter.id)
    }

    fn draft_dir(projects_root: &std::path::Path, project_id: &str, chapter_id: &str) -> PathBuf {
        projects_root
            .join(project_id)
            .join("app-meta/chapter-drafts")
            .join(chapter_id)
    }

    /// 归档区（`chapter-draft-kept/<chapter>/<stamp>/`）里最近一次归档的正文。
    ///
    /// Issue #1062 评论 7194373427 第 1 项：kept 草稿搬出可写槽位后，正文必须
    /// 仍能在归档区读到。
    fn kept_draft_text(
        projects_root: &std::path::Path,
        project_id: &str,
        chapter_id: &str,
    ) -> Option<String> {
        let chapter_root = projects_root
            .join(project_id)
            .join("app-meta/chapter-draft-kept")
            .join(chapter_id);
        let entries = std::fs::read_dir(chapter_root).ok()?;
        for entry in entries.flatten() {
            let text_path = entry.path().join("text.txt");
            if text_path.is_file() {
                return std::fs::read_to_string(text_path).ok();
            }
        }
        None
    }

    /// 模拟旧版本已经写盘的“未证明新正文”草稿：章节持久 revision=30，
    /// 编辑器本次从 0 打开后 revision=1，且没有逐字事实。
    #[allow(clippy::too_many_arguments)]
    fn write_unproven_draft(
        projects_root: &std::path::Path,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        text: &str,
        base_generation: &str,
        base_revision: u64,
        base_sha256: &str,
    ) {
        let dir = draft_dir(projects_root, project_id, chapter_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("text.txt"), text).unwrap();
        let meta = serde_json::json!({
            "schemaVersion": 2,
            "projectId": project_id,
            "volumeId": volume_id,
            "chapterId": chapter_id,
            "workspaceKey": "workspace",
            "generation": base_generation,
            "sessionId": 1,
            "baseGeneration": base_generation,
            "baseRevision": base_revision,
            "baseSha256": base_sha256,
            "textRevision": 1,
            "textSha256": content_digest_of_text(text),
            "updatedAtMs": 1,
            "facts": [],
            "ackedEventIds": [],
            "ackedThroughRevision": 0,
            "appliedTransactions": [],
            "editorBaseRevision": 0,
            "editorBaseSha256": "editor-base-sha",
            "editorBaseSession": 1,
            "committedTextRevision": 0,
            "committedTextSha256": ""
        });
        std::fs::write(
            dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
    }

    fn context(data_root: &std::path::Path, projects_root: &std::path::Path) -> ChapterSaveContext {
        ChapterSaveContext::new(
            &data_root.to_string_lossy(),
            &projects_root.to_string_lossy(),
            &writer_core::storage::GitRepoLayout {
                worktree_root: data_root.to_path_buf(),
                git_dir: data_root.join(".git"),
            },
        )
    }

    /// Issue #1062 评论 7193130307 第 1 项：无事实、编辑器 revision 未前进但
    /// 正文不是 live 内容时，恢复必须保留草稿且不调用 Core 保存。
    #[test]
    fn unproven_new_text_is_never_committed_by_restore() {
        let (temp, api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        let opened = api
            .open_chapter(&project_id, &volume_id, &chapter_id)
            .unwrap();
        assert_eq!(opened.version.revision, 30);
        write_unproven_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            "未证明新正文",
            &opened.version.object_generation,
            opened.version.revision,
            &opened.version.content_sha256,
        );

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project_id.clone(),
            },
        );

        let opened = state
            .api_for(&context(temp.path(), &projects_root))
            .open_chapter(&project_id, &volume_id, &chapter_id)
            .unwrap();
        assert_eq!(
            opened.content, "已提交正文 30",
            "Core 正文不能被未证明草稿覆盖"
        );
        assert!(
            !draft_dir(&projects_root, &project_id, &chapter_id).exists(),
            "保留草稿必须搬出可写槽位，编辑器新会话不能复用"
        );
        assert_eq!(
            kept_draft_text(&projects_root, &project_id, &chapter_id).as_deref(),
            Some("未证明新正文"),
            "归档区必须保留原草稿正文"
        );
        assert!(
            chapter_draft::kept_drafts(&projects_root, &project_id).contains(&chapter_id),
            "归档区必须能列出该章节"
        );
        let status = draft_restore_status(&project_id);
        assert_eq!(status.kept, 1);
        assert_eq!(status.kept_chapters, vec![chapter_id.clone()]);
        assert!(
            status.message.contains("draft_text_without_proof"),
            "unexpected restore message: {}",
            status.message
        );
    }

    /// Issue #1062 评论 7193130307 第 2 项：崩溃后只剩“已确认正文 + 无事实”
    /// 的目录时，按当前 Core 版本摘要幂等清理，不算未证明新编辑。
    #[test]
    fn committed_text_matching_live_version_is_cleaned_without_save() {
        let (temp, api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        let opened = api
            .open_chapter(&project_id, &volume_id, &chapter_id)
            .unwrap();
        write_unproven_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            &opened.content,
            &opened.version.object_generation,
            opened.version.revision,
            &opened.version.content_sha256,
        );

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project_id.clone(),
            },
        );

        assert!(
            !draft_dir(&projects_root, &project_id, &chapter_id).exists(),
            "与 live 一致的已确认正文应清理草稿，不产生缺证明候选"
        );
        let reopened = state
            .api_for(&context(temp.path(), &projects_root))
            .open_chapter(&project_id, &volume_id, &chapter_id)
            .unwrap();
        assert_eq!(reopened.content, opened.content);
        assert_eq!(reopened.version.revision, opened.version.revision);
    }

    /// Issue #1062 评论 7193130307 第 2 项：保存成功并 ack 后，同一份已确认
    /// 正文必须收口，不能留下“无事实 + 旧正文”的僵尸草稿。
    #[test]
    fn committed_text_is_settled_and_draft_directory_removed() {
        let (temp, api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        let root = projects_root.as_path();
        let text = "已提交正文";
        let digest = content_digest_of_text(text);
        chapter_draft::note_text(
            root,
            &project_id,
            &chapter_id,
            &volume_id,
            1,
            "gen-1",
            text.to_string(),
            1,
            &digest,
            ("gen-1", 30, "base-sha"),
            0,
        );
        chapter_draft::note_fact(
            root,
            &project_id,
            &chapter_id,
            &volume_id,
            1,
            "gen-1",
            chapter_draft::DraftFact {
                event_id: "e1".to_string(),
                revision: 1,
                session_id: 1,
                projection_json: serde_json::json!({
                    "eventId": "e1",
                    "revision": 1,
                    "event": {"proof": {"afterTextSha256": digest}}
                })
                .to_string(),
            },
        )
        .unwrap();
        chapter_draft::persist_current(root, &project_id, &chapter_id, true).unwrap();

        // 正文收口与事实 ack 必须在同一个 meta 决议里完成。
        assert!(
            chapter_draft::mark_committed_with_settlement(
                root,
                &project_id,
                &chapter_id,
                "tx-1",
                &["e1".to_string()],
                "gen-1",
                31,
                "after-sha",
                Some(&chapter_draft::TextSettlement {
                    session_id: 1,
                    snapshot_revision: 1,
                    snapshot_digest: digest.clone(),
                }),
                1,
            )
            .unwrap()
            .removed(),
            "正文已收口且事实全部 ack：整章草稿应删除"
        );
        assert!(
            !draft_dir(&projects_root, &project_id, &chapter_id).exists(),
            "已提交正文收口且事实 ack 后应删除整章草稿"
        );
    }

    /// Issue #1062 评论 7193130307 第 2 项：保存 A 期间已输入 B 时只清 A。
    #[test]
    fn settle_keeps_newer_text_arrived_during_save() {
        let (_temp, _api, project_id, volume_id, chapter_id) = workspace();
        let temp2 = tempfile::tempdir().unwrap();
        let root = temp2.path();
        let digest_a = content_digest_of_text("A");
        let digest_b = content_digest_of_text("B");
        chapter_draft::note_text(
            root,
            &project_id,
            &chapter_id,
            &volume_id,
            1,
            "gen-1",
            "A".to_string(),
            1,
            &digest_a,
            ("gen-1", 30, "base-sha"),
            0,
        );
        chapter_draft::note_text(
            root,
            &project_id,
            &chapter_id,
            &volume_id,
            1,
            "gen-1",
            "B".to_string(),
            2,
            &digest_b,
            ("gen-1", 30, "base-sha"),
            0,
        );

        assert!(!chapter_draft::mark_committed_with_settlement(
            root,
            &project_id,
            &chapter_id,
            "tx-1",
            &[],
            "gen-1",
            31,
            "after-sha",
            Some(&chapter_draft::TextSettlement {
                session_id: 1,
                snapshot_revision: 1,
                snapshot_digest: digest_a.clone(),
            }),
            1,
        )
        .expect("ack")
        .removed());
        let snapshot = chapter_draft::snapshot(root, &project_id, &chapter_id).unwrap();
        assert_eq!(snapshot.text.as_deref(), Some("B"));
        assert_eq!(snapshot.text_revision, 2);
    }
}

mod stale_base_cleanup_tests {
    use super::super::draft_jobs::draft_restore_status;
    use super::super::restore_draft::handle_restore_project;
    use super::super::worker::ChapterSaveWorkerState;
    use super::super::{ChapterRestoreCommand, ChapterSaveContext};
    use writer_core::api::WriterCoreApi;

    fn context(data_root: &std::path::Path, projects_root: &std::path::Path) -> ChapterSaveContext {
        ChapterSaveContext::new(
            &data_root.to_string_lossy(),
            &projects_root.to_string_lossy(),
            &writer_core::storage::GitRepoLayout {
                worktree_root: data_root.to_path_buf(),
                git_dir: data_root.join(".git"),
            },
        )
    }

    /// Issue #1062 评论 7193847871 第 3 项：草稿正文已经等于 live 正文，但
    /// 草稿记录的 base 还是旧版本（异常退出未推进）时，恢复必须幂等清理，
    /// 不能报 base_unrecoverable 永久保留。
    #[test]
    fn stale_base_with_live_text_is_cleaned_without_save() {
        let temp = tempfile::tempdir().unwrap();
        let projects_root = temp.path().join("projects");
        let api = WriterCoreApi::new(temp.path(), projects_root.clone());
        let project = api.create_project("作品").unwrap();
        let volumes = api.list_volumes(&project.id).unwrap();
        let volume_id = volumes.first().unwrap().id.clone();
        let chapter = api
            .create_chapter(&project.id, &volume_id, "第一章")
            .unwrap();
        let live = "Core 已提交正文";
        api.save_chapter_content_versioned(&project.id, &volume_id, &chapter.id, live, None, None)
            .unwrap();
        let opened = api
            .open_chapter(&project.id, &volume_id, &chapter.id)
            .unwrap();
        let dir = projects_root
            .join(&project.id)
            .join("app-meta/chapter-drafts")
            .join(&chapter.id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("text.txt"), live).unwrap();
        let meta = serde_json::json!({
            "schemaVersion": 2,
            "projectId": project.id,
            "volumeId": volume_id,
            "chapterId": chapter.id,
            "workspaceKey": "workspace",
            "generation": "gen-stale",
            "sessionId": 1,
            "baseGeneration": "gen-stale",
            "baseRevision": 7,
            "baseSha256": "stale-base-sha",
            "textRevision": 1,
            "textSha256": writer_core::writing_stats::proof::content_digest_of_text(live),
            "updatedAtMs": 1,
            "facts": [],
            "ackedEventIds": [],
            "ackedThroughRevision": 0,
            "appliedTransactions": [],
            "editorBaseRevision": 0,
            "editorBaseSha256": "",
            "editorBaseSession": 1,
            "committedTextRevision": 0,
            "committedTextSha256": ""
        });
        std::fs::write(
            dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project.id.clone(),
            },
        );

        assert!(
            !dir.exists(),
            "已提交正文与 live 一致时应清理草稿，不能因 stale base 保留"
        );
        let reopened = state
            .api_for(&context(temp.path(), &projects_root))
            .open_chapter(&project.id, &volume_id, &chapter.id)
            .unwrap();
        assert_eq!(reopened.content, live);
        assert_eq!(reopened.version.revision, opened.version.revision);
        let status = draft_restore_status(&project.id);
        assert_eq!(status.kept, 0);
    }
}

mod restore_isolation_tests {
    //! Issue #1062 评论 7195690539 第 1/2/3 项：恢复结果的按章隔离契约。

    use super::super::draft_jobs::draft_restore_status;
    use super::super::restore_draft::handle_restore_project;
    use super::super::worker::ChapterSaveWorkerState;
    use super::super::{ChapterRestoreCommand, ChapterSaveContext};
    use crate::backend::chapter_draft;
    use std::path::PathBuf;
    use writer_core::api::WriterCoreApi;
    use writer_core::writing_stats::proof::content_digest_of_text;

    fn workspace() -> (tempfile::TempDir, WriterCoreApi, String, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let projects_root = temp.path().join("projects");
        let api = WriterCoreApi::new(temp.path(), projects_root.clone());
        let project = api.create_project("作品").unwrap();
        let volumes = api.list_volumes(&project.id).unwrap();
        let volume_id = volumes.first().unwrap().id.clone();
        let chapter = api
            .create_chapter(&project.id, &volume_id, "第一章")
            .unwrap();
        api.save_chapter_content_versioned(
            &project.id,
            &volume_id,
            &chapter.id,
            "已提交正文",
            None,
            None,
        )
        .unwrap();
        (temp, api, project.id, volume_id, chapter.id)
    }

    fn context(data_root: &std::path::Path, projects_root: &std::path::Path) -> ChapterSaveContext {
        ChapterSaveContext::new(
            &data_root.to_string_lossy(),
            &projects_root.to_string_lossy(),
            &writer_core::storage::GitRepoLayout {
                worktree_root: data_root.to_path_buf(),
                git_dir: data_root.join(".git"),
            },
        )
    }

    fn draft_dir(projects_root: &std::path::Path, project_id: &str, chapter_id: &str) -> PathBuf {
        projects_root
            .join(project_id)
            .join("app-meta/chapter-drafts")
            .join(chapter_id)
    }

    fn write_verified_draft(
        projects_root: &std::path::Path,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        text: &str,
    ) {
        // 无逐字事实、只有正文快照：编辑器基线摘要与正文一致，允许发布。
        // 这正是评论 #7195690539 第 2 项的“facts/pending 均为空”场景。
        let digest = content_digest_of_text(text);
        chapter_draft::note_text(
            projects_root,
            project_id,
            chapter_id,
            volume_id,
            1,
            "gen-1",
            text.to_string(),
            1,
            &digest,
            ("gen-1", 30, "base-sha"),
            1,
        );
        chapter_draft::persist_current(projects_root, project_id, chapter_id, true)
            .expect("persist");
    }

    /// 缺失正文 blob 必须作为“损坏草稿”进入恢复队列并归档，不能表现为空草稿。
    #[test]
    fn missing_text_blob_is_archived_and_does_not_open_a_fresh_slot() {
        let (temp, api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        write_verified_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            "未提交正文",
        );
        let dir = draft_dir(&projects_root, &project_id, &chapter_id);
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json")).unwrap()).unwrap();
        let blob = meta["textBlob"].as_str().unwrap().to_string();
        std::fs::remove_file(dir.join("texts").join(&blob)).unwrap();

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project_id.clone(),
            },
        );

        assert!(!dir.exists(), "损坏草稿必须离开可写槽位");
        let status = draft_restore_status(&project_id);
        assert_eq!(status.kept, 1);
        assert!(status.kept_chapters.contains(&chapter_id));
        assert!(status.archived_chapters.contains(&chapter_id));
        assert!(status.blocked_chapters.is_empty());
    }

    /// 只有损坏 meta.json 的目录同样是未决事务：必须被识别并归档。
    #[test]
    fn meta_only_corrupt_draft_is_archived() {
        let (temp, api, project_id, _volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        let dir = draft_dir(&projects_root, &project_id, &chapter_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("meta.json"), b"{ not-json").unwrap();

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project_id.clone(),
            },
        );

        assert!(!dir.exists(), "损坏 meta 目录必须搬出可写槽位");
        let status = draft_restore_status(&project_id);
        assert_eq!(status.kept, 1);
        assert!(status.archived_chapters.contains(&chapter_id));
    }

    /// meta JSON 合法但身份/版本损坏：同样是未决事务，必须归档。
    #[test]
    fn meta_schema_mismatch_is_archived() {
        let (temp, api, project_id, _volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        let dir = draft_dir(&projects_root, &project_id, &chapter_id);
        std::fs::create_dir_all(&dir).unwrap();
        let meta = serde_json::json!({
            "schemaVersion": 99,
            "projectId": project_id,
            "chapterId": chapter_id,
        });
        std::fs::write(dir.join("meta.json"), meta.to_string()).unwrap();

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project_id.clone(),
            },
        );

        assert!(!dir.exists(), "身份/版本损坏的 meta 目录必须搬出可写槽位");
        let status = draft_restore_status(&project_id);
        assert!(status.archived_chapters.contains(&chapter_id));
    }

    /// 归档失败必须按章节阻断，原目录保持不动，QML 据此拒绝打开该章。
    #[test]
    fn archive_failure_blocks_only_that_chapter() {
        let (temp, api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        // 让归档根成为普通文件：create_dir_all / rename 必然失败。
        let kept_root = projects_root
            .join(&project_id)
            .join("app-meta")
            .join("chapter-draft-kept");
        std::fs::create_dir_all(kept_root.parent().unwrap()).unwrap();
        std::fs::write(&kept_root, b"blocked").unwrap();
        // 无事实、正文不是 live：走“外部变更/未证明保留”分支（永久归档）。
        chapter_draft::note_text(
            &projects_root,
            &project_id,
            &chapter_id,
            &volume_id,
            1,
            "gen-1",
            "未证明正文".to_string(),
            1,
            &content_digest_of_text("未证明正文"),
            ("gen-1", 30, "base-sha"),
            1,
        );
        chapter_draft::persist_current(&projects_root, &project_id, &chapter_id, true)
            .expect("persist unproven draft");

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project_id.clone(),
            },
        );

        assert!(
            draft_dir(&projects_root, &project_id, &chapter_id).exists(),
            "归档失败时原目录必须保持原样"
        );
        let status = draft_restore_status(&project_id);
        assert!(status.blocked_chapters.contains(&chapter_id));
        assert!(status.kept_chapters.is_empty());
        assert!(status.archived_chapters.is_empty());
    }

    /// 损坏的 pending-inputs spool 代表未转换的原始编辑事实：必须保留重试，
    /// 不能被当成空草稿结算删除（评论 #7209683560 第 2 项）。
    #[test]
    fn corrupt_pending_spool_is_kept_for_retry() {
        let (temp, api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        write_verified_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            "带 spool 的正文",
        );
        let dir = draft_dir(&projects_root, &project_id, &chapter_id);
        let spool = dir.join("pending-inputs");
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::write(spool.join("bad-input.json"), b"{ not-json").unwrap();

        let mut state = ChapterSaveWorkerState::for_tests(api);
        handle_restore_project(
            &mut state,
            ChapterRestoreCommand {
                context: context(temp.path(), &projects_root),
                project_id: project_id.clone(),
            },
        );

        assert!(!dir.exists(), "损坏 spool 的草稿必须离开可写槽位");
        let status = draft_restore_status(&project_id);
        assert!(status.retry_chapters.contains(&chapter_id));
        assert!(status.archived_chapters.is_empty());
        // 归档副本里必须仍然保留原始 spool 文件，等待修复后重试。
        let kept = projects_root
            .join(&project_id)
            .join("app-meta/chapter-draft-kept")
            .join(&chapter_id);
        let mut found = false;
        for entry in std::fs::read_dir(&kept).unwrap().flatten() {
            if entry.path().join("pending-inputs/bad-input.json").is_file() {
                found = true;
            }
        }
        assert!(found, "归档副本必须保留损坏的原始 spool");
    }

    /// 可自动重试事务的独立槽位：归档 + 标记，再搬回活动槽位重试。
    #[test]
    fn retry_slot_roundtrip_and_active_conflict() {
        let (temp, _api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        write_verified_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            "重试正文",
        );
        let archived = chapter_draft::quarantine(&projects_root, &project_id, &chapter_id)
            .expect("quarantine")
            .expect("archived path");
        chapter_draft::record_retry(
            &projects_root,
            &project_id,
            &chapter_id,
            &volume_id,
            std::path::Path::new(&archived),
            "unit retry",
            0,
        )
        .expect("record retry");
        assert_eq!(
            chapter_draft::retry_chapters(&projects_root, &project_id),
            vec![chapter_id.clone()]
        );
        assert_eq!(
            chapter_draft::materialize_retry(&projects_root, &project_id, &chapter_id)
                .expect("materialize"),
            chapter_draft::RetryMaterialize::Moved
        );
        assert!(draft_dir(&projects_root, &project_id, &chapter_id).exists());
        assert!(chapter_draft::retry_chapters(&projects_root, &project_id).is_empty());

        // 活动槽位已有新会话草稿时，可重试副本绝不覆盖。
        let archived_again = chapter_draft::quarantine(&projects_root, &project_id, &chapter_id)
            .expect("quarantine again")
            .expect("archived again");
        write_verified_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            "新会话正文",
        );
        chapter_draft::record_retry(
            &projects_root,
            &project_id,
            &chapter_id,
            &volume_id,
            std::path::Path::new(&archived_again),
            "unit retry conflict",
            0,
        )
        .expect("record retry again");
        assert_eq!(
            chapter_draft::materialize_retry(&projects_root, &project_id, &chapter_id)
                .expect("materialize conflict"),
            chapter_draft::RetryMaterialize::ActiveDraftExists
        );
        assert_eq!(
            chapter_draft::retry_chapters(&projects_root, &project_id),
            vec![chapter_id]
        );
    }

    /// 评论 #7210360212 第 2 项：重试意图先写进草稿目录再 rename；外部索引
    /// 丢失（rename 后崩溃）也能从归档目录里的意图重建并继续自动重试。
    #[test]
    fn retry_intent_survives_missing_external_index() {
        let (temp, _api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        write_verified_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            "待自动重试正文",
        );
        let mut counters = super::super::kept_draft::RestoreCounters::default();
        counters.keep(
            &projects_root,
            &project_id,
            &chapter_id,
            super::super::kept_draft::KeptDisposition::Retry {
                volume_id: volume_id.clone(),
                reason: "unit retry".to_string(),
                attempts: 0,
            },
        );
        assert!(!draft_dir(&projects_root, &project_id, &chapter_id).exists());
        // 模拟崩溃窗口：删掉外部索引，只留归档目录内部的 retry-intent.json。
        for candidate in chapter_draft::list_retries(&projects_root, &project_id) {
            assert!(!candidate.intent_backed, "index written in normal path");
            chapter_draft::remove_marker(&candidate.marker_path).unwrap();
        }
        let rebuilt = chapter_draft::list_retries(&projects_root, &project_id);
        assert_eq!(rebuilt.len(), 1, "intent rebuilds the retry queue");
        assert!(rebuilt[0].intent_backed);

        assert_eq!(
            chapter_draft::materialize_retry(&projects_root, &project_id, &chapter_id).unwrap(),
            chapter_draft::RetryMaterialize::Moved
        );
        assert!(draft_dir(&projects_root, &project_id, &chapter_id).exists());
        assert!(chapter_draft::retry_chapters(&projects_root, &project_id).is_empty());
    }

    /// 评论 #7210163429 第 1 项：新草稿 B 结算成功，不能连带删除旧草稿 A 的
    /// 重试标记；下一次进入项目仍应从 A 的标记 materialize。
    #[test]
    fn settling_new_draft_keeps_older_transaction_retry_marker() {
        let (temp, _api, project_id, volume_id, chapter_id) = workspace();
        let projects_root = temp.path().join("projects");
        let archived_a = projects_root
            .join(&project_id)
            .join("app-meta/chapter-draft-kept")
            .join(&chapter_id)
            .join("stamp-a");
        std::fs::create_dir_all(&archived_a).unwrap();
        std::fs::write(archived_a.join("text.txt"), "旧草稿 A").unwrap();
        chapter_draft::record_retry(
            &projects_root,
            &project_id,
            &chapter_id,
            &volume_id,
            &archived_a,
            "first",
            0,
        )
        .unwrap();
        write_verified_draft(
            &projects_root,
            &project_id,
            &volume_id,
            &chapter_id,
            "新正文 B",
        );

        let mut counters = super::super::kept_draft::RestoreCounters::default();
        assert!(counters.settle_removed(&projects_root, &project_id, &chapter_id));
        assert!(!draft_dir(&projects_root, &project_id, &chapter_id).exists());
        assert_eq!(
            chapter_draft::retry_chapters(&projects_root, &project_id),
            vec![chapter_id.clone()]
        );
        assert_eq!(
            chapter_draft::list_retries(&projects_root, &project_id).len(),
            1
        );
    }
}
