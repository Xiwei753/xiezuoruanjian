//! 搜索 API 已迁移到 per-WriterCore 实例持有。
//! 本文件仅保留测试用例。

#[cfg(test)]
mod tests {
    use crate::search::service::SearchIndexService;
    use crate::search::types::*;
    use tempfile::TempDir;

    fn rebuild_index(workspace: &std::path::Path, project_id: Option<&str>) -> crate::error::Result<SearchIndexStatus> {
        let entries = crate::search::rebuild::rebuild_index(workspace, project_id)?;
        let mut service = SearchIndexService::new();
        service.rebuild_from_entries(entries);
        Ok(service.status())
    }

    fn search_with_service(service: &mut SearchIndexService, query: &str, scope: SearchScope, limit: usize, cursor: Option<&str>) -> Vec<SearchResult> {
        service.search(query, scope, limit, cursor)
    }

    #[test]
    fn search_empty_query_returns_nothing() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_index(dir.path(), None);
        let mut service = SearchIndexService::new();
        let results = search_with_service(&mut service, "", SearchScope::All, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_nonexistent_query_returns_nothing() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_index(dir.path(), None);
        let mut service = SearchIndexService::new();
        let results = search_with_service(&mut service, "nonexistent", SearchScope::All, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_index_status_default() {
        let service = SearchIndexService::new();
        let status = service.status();
        assert_eq!(status.total_entries, 0);
    }

    #[test]
    fn enqueue_and_search_update() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "test:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: "Test Chapter".to_string(),
            body: "Test Chapter".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });

        let results = search_with_service(&mut service, "Test", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test Chapter");
        assert_eq!(results[0].object_id, "test:1");
    }

    #[test]
    fn search_delete_removes_entry() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "del:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: "Delete Me".to_string(),
            body: "Delete Me".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });

        let results = search_with_service(&mut service, "Delete", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);

        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Delete,
            object_id: "del:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: String::new(),
            body: String::new(),
            target: None,
        });

        let results = search_with_service(&mut service, "Delete", SearchScope::ChapterTitle, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_chinese_text_no_panic() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "zh:1".to_string(),
            scope: SearchScope::ChapterBody,
            title: "中文章节".to_string(),
            body: "这是一段中文正文内容，用于测试搜索功能是否能在多字节字符上正确工作而不发生崩溃。".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });

        let results = search_with_service(&mut service, "中文", SearchScope::ChapterBody, 10, None);
        assert!(!results.is_empty());
        assert!(results[0].summary.contains("中文"));
    }

    #[test]
    fn search_stable_pagination() {
        let mut service = SearchIndexService::new();
        for i in 0..5 {
            service.enqueue_update(SearchIndexUpdate {
                action: SearchIndexAction::Upsert,
                object_id: format!("page:{}", i),
                scope: SearchScope::ChapterTitle,
                title: format!("Alpha {}", i),
                body: format!("Alpha {}", i),
                target: Some(SearchTarget {
                    project_id: Some("p1".to_string()),
                    volume_id: None,
                    chapter_id: Some(format!("c{}", i)),
                    starmap_id: None,
                    node_id: None,
                    setting_key: None,
                }),
            });
        }

        let page1 = search_with_service(&mut service, "Alpha", SearchScope::ChapterTitle, 2, None);
        assert_eq!(page1.len(), 2);

        let cursor = page1.last().unwrap().object_id.clone();
        let page2 = search_with_service(&mut service, "Alpha", SearchScope::ChapterTitle, 2, Some(&cursor));
        assert_eq!(page2.len(), 2);

        let all_ids: Vec<String> = page1.iter().chain(page2.iter()).map(|r| r.object_id.clone()).collect();
        let unique_ids: std::collections::HashSet<&String> = all_ids.iter().collect();
        assert_eq!(unique_ids.len(), 4);
    }

    #[test]
    fn search_reinsert_dedup_scope_index() {
        let mut service = SearchIndexService::new();
        for _ in 0..3 {
            service.enqueue_update(SearchIndexUpdate {
                action: SearchIndexAction::Upsert,
                object_id: "dup:1".to_string(),
                scope: SearchScope::ChapterTitle,
                title: "Duplicate".to_string(),
                body: "Duplicate".to_string(),
                target: Some(SearchTarget {
                    project_id: Some("p1".to_string()),
                    volume_id: None,
                    chapter_id: Some("c1".to_string()),
                    starmap_id: None,
                    node_id: None,
                    setting_key: None,
                }),
            });
        }

        let results = search_with_service(&mut service, "Duplicate", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_scope_change_removes_old_scope_entry() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "scope:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: "ScopeTest".to_string(),
            body: "ScopeTest".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });

        let title_results = search_with_service(&mut service, "ScopeTest", SearchScope::ChapterTitle, 10, None);
        assert_eq!(title_results.len(), 1);
        let body_results = search_with_service(&mut service, "ScopeTest", SearchScope::ChapterBody, 10, None);
        assert!(body_results.is_empty());

        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "scope:1".to_string(),
            scope: SearchScope::ChapterBody,
            title: "ScopeTest".to_string(),
            body: "ScopeTest".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });

        let title_results_after = search_with_service(&mut service, "ScopeTest", SearchScope::ChapterTitle, 10, None);
        assert!(title_results_after.is_empty());
        let body_results_after = search_with_service(&mut service, "ScopeTest", SearchScope::ChapterBody, 10, None);
        assert_eq!(body_results_after.len(), 1);
    }

    #[test]
    fn search_starmap_link_scope_works() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "link:1".to_string(),
            scope: SearchScope::StarmapLink,
            title: "ChapterRef".to_string(),
            body: "ChapterRef".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: None,
                starmap_id: Some("sm1".to_string()),
                node_id: None,
                setting_key: None,
            }),
        });

        let results = search_with_service(&mut service, "ChapterRef", SearchScope::StarmapLink, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].scope, SearchScope::StarmapLink);
    }

    #[test]
    fn extract_starmap_link_entries() {
        use crate::search::extractor::extract_starmap_entries;
        let dir = TempDir::new().unwrap();
        let starmaps_root = dir.path().join("app-meta").join("starmaps");
        let starmap_dir = starmaps_root.join("sm1");
        std::fs::create_dir_all(starmap_dir.join("links")).unwrap();
        std::fs::write(
            starmap_dir.join("graph.json"),
            serde_json::json!({"title": "TestMap", "linkIds": []}).to_string(),
        ).unwrap();
        std::fs::write(
            starmaps_root.join("sm1.meta.json"),
            serde_json::json!({"starmapId": "sm1", "projectId": "p1", "title": "TestMap", "createdAt": 0, "updatedAt": 0}).to_string(),
        ).unwrap();
        let link = crate::starmap::types::StarMapLink {
            link_id: "l1".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Node { node_id: "n1".to_string() },
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: "sm1".to_string(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: Some("MyLink".to_string()),
            created_at: 0,
            updated_at: 0,
        };
        std::fs::write(
            starmap_dir.join("links").join("l1.json"),
            serde_json::to_string(&link).unwrap(),
        ).unwrap();

        let entries = extract_starmap_entries(dir.path(), None).unwrap();
        let link_entries: Vec<_> = entries.iter().filter(|e| e.scope == SearchScope::StarmapLink).collect();
        assert_eq!(link_entries.len(), 1);
        assert_eq!(link_entries[0].title, "MyLink");
    }

    #[test]
    fn enqueue_update_applies_immediately() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "imm:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: "Immediate".to_string(),
            body: "Immediate".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });
        let results = search_with_service(&mut service, "Immediate", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Immediate");
    }

    #[test]
    fn remove_by_prefix_deletes_matching_entries() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap:s1".to_string(),
            scope: SearchScope::StarmapTitle,
            title: "Map1".to_string(),
            body: "Map1".to_string(),
            target: Some(SearchTarget {
                project_id: None,
                volume_id: None,
                chapter_id: None,
                starmap_id: Some("s1".to_string()),
                node_id: None,
                setting_key: None,
            }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap_node:s1:n1".to_string(),
            scope: SearchScope::StarmapNode,
            title: "Node1".to_string(),
            body: "Node1".to_string(),
            target: Some(SearchTarget {
                project_id: None,
                volume_id: None,
                chapter_id: None,
                starmap_id: Some("s1".to_string()),
                node_id: Some("n1".to_string()),
                setting_key: None,
            }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap:s2".to_string(),
            scope: SearchScope::StarmapTitle,
            title: "Map2".to_string(),
            body: "Map2".to_string(),
            target: Some(SearchTarget {
                project_id: None,
                volume_id: None,
                chapter_id: None,
                starmap_id: Some("s2".to_string()),
                node_id: None,
                setting_key: None,
            }),
        });

        let results = search_with_service(&mut service, "Map", SearchScope::All, 10, None);
        assert_eq!(results.len(), 2);

        service.remove_by_prefix("starmap_node:s1:");
        let node_results = search_with_service(&mut service, "Node", SearchScope::StarmapNode, 10, None);
        assert!(node_results.is_empty());

        let map_results = search_with_service(&mut service, "Map", SearchScope::StarmapTitle, 10, None);
        assert_eq!(map_results.len(), 2);
    }

    #[test]
    fn delete_starmap_removes_all_child_indices_by_prefix() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap:s1".to_string(),
            scope: SearchScope::StarmapTitle,
            title: "Map1".to_string(),
            body: "Map1".to_string(),
            target: Some(SearchTarget { project_id: None, volume_id: None, chapter_id: None, starmap_id: Some("s1".to_string()), node_id: None, setting_key: None }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap_node:s1:n1".to_string(),
            scope: SearchScope::StarmapNode,
            title: "Node1".to_string(),
            body: "Node1".to_string(),
            target: Some(SearchTarget { project_id: None, volume_id: None, chapter_id: None, starmap_id: Some("s1".to_string()), node_id: Some("n1".to_string()), setting_key: None }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap_node:s1:n2".to_string(),
            scope: SearchScope::StarmapNode,
            title: "Node2".to_string(),
            body: "Node2".to_string(),
            target: Some(SearchTarget { project_id: None, volume_id: None, chapter_id: None, starmap_id: Some("s1".to_string()), node_id: Some("n2".to_string()), setting_key: None }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap_edge:s1:e1".to_string(),
            scope: SearchScope::StarmapEdgeLabel,
            title: "Edge1".to_string(),
            body: "Edge1".to_string(),
            target: Some(SearchTarget { project_id: None, volume_id: None, chapter_id: None, starmap_id: Some("s1".to_string()), node_id: None, setting_key: None }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap_link:s1:l1".to_string(),
            scope: SearchScope::StarmapLink,
            title: "Link1".to_string(),
            body: "Link1".to_string(),
            target: Some(SearchTarget { project_id: None, volume_id: None, chapter_id: None, starmap_id: Some("s1".to_string()), node_id: None, setting_key: None }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap_hyperlink:s1:h1".to_string(),
            scope: SearchScope::StarmapHyperlink,
            title: "HL1".to_string(),
            body: "HL1".to_string(),
            target: Some(SearchTarget { project_id: None, volume_id: None, chapter_id: None, starmap_id: Some("s1".to_string()), node_id: None, setting_key: None }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap_embed:s1:em1".to_string(),
            scope: SearchScope::StarmapEmbed,
            title: "Embed1".to_string(),
            body: "Embed1".to_string(),
            target: Some(SearchTarget { project_id: None, volume_id: None, chapter_id: None, starmap_id: Some("s1".to_string()), node_id: None, setting_key: None }),
        });

        assert_eq!(service.status().total_entries, 7);

        for prefix in &[
            "starmap:s1".to_string(),
            "starmap_node:s1:".to_string(),
            "starmap_edge:s1:".to_string(),
            "starmap_hyperlink:s1:".to_string(),
            "starmap_link:s1:".to_string(),
            "starmap_embed:s1:".to_string(),
        ] {
            service.remove_by_prefix(prefix);
        }

        assert_eq!(service.status().total_entries, 0);
    }

    #[test]
    fn node_content_search_text_handles_all_types() {
        use crate::starmap::semantic::StarMapNodeContent;

        let inline = StarMapNodeContent::Inline {
            summary: Some("summary".to_string()),
            body: Some("body text".to_string()),
        };
        assert_eq!(inline.search_text(), "summary body text");

        let chapter_ref = StarMapNodeContent::ChapterRef {
            project_id: "p1".to_string(),
            volume_id: None,
            chapter_id: "ch1".to_string(),
            range_start: None,
            range_end: None,
        };
        assert_eq!(chapter_ref.search_text(), "ch1");

        let entity_ref = StarMapNodeContent::EntityRef {
            entity_type: "character".to_string(),
            entity_id: "e1".to_string(),
        };
        assert_eq!(entity_ref.search_text(), "character e1");

        let external_ref = StarMapNodeContent::ExternalRef {
            uri: "https://example.com".to_string(),
            label: Some("Example".to_string()),
        };
        assert_eq!(external_ref.search_text(), "Example https://example.com");

        let empty = StarMapNodeContent::Empty;
        assert_eq!(empty.search_text(), "");
    }

    #[test]
    fn rebuild_extractor_uses_structured_node_content() {
        use crate::search::extractor::extract_starmap_entries;
        let dir = TempDir::new().unwrap();
        let starmaps_root = dir.path().join("app-meta").join("starmaps");
        let starmap_dir = starmaps_root.join("sm1");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::write(
            starmap_dir.join("graph.json"),
            serde_json::json!({"title": "TestMap"}).to_string(),
        ).unwrap();
        std::fs::write(
            starmaps_root.join("sm1.meta.json"),
            serde_json::json!({"starmapId": "sm1", "projectId": "p1", "title": "TestMap", "createdAt": 0, "updatedAt": 0}).to_string(),
        ).unwrap();
        std::fs::write(
&starmap_dir.join("nodes").join("n1.json"),
            serde_json::json!({
                "id": "n1",
                "title": "MyNode",
                "kind": "note",
                "content": {"type": "inline", "summary": "节点摘要", "body": "节点正文"},
                "tags": ["标签A", "标签B"],
                "createdAt": 0,
                "updatedAt": 0
            }).to_string(),
        ).unwrap();

        let entries = extract_starmap_entries(dir.path(), None).unwrap();
        let node_entries: Vec<_> = entries.iter().filter(|e| e.scope == SearchScope::StarmapNode).collect();
        assert_eq!(node_entries.len(), 1);
        assert_eq!(node_entries[0].title, "MyNode");
        assert!(node_entries[0].body.contains("节点摘要"));
        assert!(node_entries[0].body.contains("节点正文"));
        assert!(node_entries[0].body.contains("标签A"));
        assert!(node_entries[0].body.contains("标签B"));
    }

    #[test]
    fn cross_entry_create_project_then_search() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("MyProject").unwrap();
        let results = api.search_service_search("MyProject", SearchScope::ProjectTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "MyProject");
        assert_eq!(results[0].target.project_id.as_deref(), Some(project.id.as_str()));
    }

    #[test]
    fn cross_entry_rename_project_then_search() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("OldName").unwrap();
        api.rename_project(&project.id, "NewName").unwrap();
        let old_results = api.search_service_search("OldName", SearchScope::ProjectTitle, 10, None);
        assert!(old_results.is_empty());
        let new_results = api.search_service_search("NewName", SearchScope::ProjectTitle, 10, None);
        assert_eq!(new_results.len(), 1);
        assert_eq!(new_results[0].title, "NewName");
    }

    #[test]
    fn cross_entry_delete_project_removes_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("ToDelete").unwrap();
        let before = api.search_service_search("ToDelete", SearchScope::ProjectTitle, 10, None);
        assert_eq!(before.len(), 1);
        api.delete_project(&project.id).unwrap();
        let after = api.search_service_search("ToDelete", SearchScope::ProjectTitle, 10, None);
        assert!(after.is_empty());
    }

    #[test]
    fn cross_entry_create_volume_then_search() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let volume = api.create_volume(&project.id, "MyVolume").unwrap();
        let results = api.search_service_search("MyVolume", SearchScope::VolumeTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "MyVolume");
        assert_eq!(results[0].target.volume_id.as_deref(), Some(volume.id.as_str()));
    }

    #[test]
    fn cross_entry_create_chapter_then_search() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let volume = api.create_volume(&project.id, "V1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "MyChapter").unwrap();
        let results = api.search_service_search("MyChapter", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "MyChapter");
        assert_eq!(results[0].target.chapter_id.as_deref(), Some(chapter.id.as_str()));
    }

    #[test]
    fn cross_entry_save_chapter_content_searchable() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let volume = api.create_volume(&project.id, "V1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch1").unwrap();
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, "Hello World").unwrap();
        let results = api.search_service_search("Hello", SearchScope::ChapterBody, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Ch1");
    }

    #[test]
    fn cross_entry_clear_chapter_content_removes_body_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let volume = api.create_volume(&project.id, "V1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "Ch1").unwrap();
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, "UniqueContent").unwrap();
        let before = api.search_service_search("UniqueContent", SearchScope::ChapterBody, 10, None);
        assert_eq!(before.len(), 1);
        api.clear_chapter_content(&project.id, &volume.id, &chapter.id).unwrap();
        let after = api.search_service_search("UniqueContent", SearchScope::ChapterBody, 10, None);
        assert!(after.is_empty());
    }

    #[test]
    fn cross_entry_rename_chapter_updates_search() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let volume = api.create_volume(&project.id, "V1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "OldTitle").unwrap();
        api.rename_chapter(&project.id, &volume.id, &chapter.id, "NewTitle").unwrap();
        let old_results = api.search_service_search("OldTitle", SearchScope::ChapterTitle, 10, None);
        assert!(old_results.is_empty());
        let new_results = api.search_service_search("NewTitle", SearchScope::ChapterTitle, 10, None);
        assert_eq!(new_results.len(), 1);
    }

    #[test]
    fn cross_entry_create_starmap_then_search() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let meta = api.create_starmap("MyStarMap", "desc", None).unwrap();
        let results = api.search_service_search("MyStarMap", SearchScope::StarmapTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "MyStarMap");
        assert_eq!(results[0].target.starmap_id.as_deref(), Some(meta.starmap_id.as_str()));
    }

    #[test]
    fn cross_entry_rebuild_matches_incremental() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("RebuildP").unwrap();
        let volume = api.create_volume(&project.id, "V1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "RebuildCh").unwrap();
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, "RebuildContent").unwrap();

        let incremental_title = api.search_service_search("RebuildCh", SearchScope::ChapterTitle, 10, None);
        let incremental_body = api.search_service_search("RebuildContent", SearchScope::ChapterBody, 10, None);
        assert_eq!(incremental_title.len(), 1);
        assert_eq!(incremental_body.len(), 1);
        assert_eq!(incremental_title[0].title, "RebuildCh");
        assert_eq!(incremental_body[0].title, "RebuildCh");

        api.search_service_rebuild(None).unwrap();
        let rebuild_title = api.search_service_search("RebuildCh", SearchScope::ChapterTitle, 10, None);
        let rebuild_body = api.search_service_search("RebuildContent", SearchScope::ChapterBody, 10, None);
        assert_eq!(rebuild_title.len(), 1);
        assert_eq!(rebuild_body.len(), 1);
        assert_eq!(rebuild_title[0].title, "RebuildCh");
        assert_eq!(rebuild_body[0].title, "RebuildCh");
    }

    #[test]
    fn cross_entry_starmap_project_id_in_incremental() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let meta = api.create_starmap("BoundMap", "desc", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let node = crate::api::types::StarMapNodeDto {
            id: String::new(),
            title: "BoundNode".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: Some("node summary".to_string()),
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let _ = api.add_starmap_node(&meta.starmap_id, node, 0.0, 0.0);

        let results = api.search_service_search("BoundNode", SearchScope::StarmapNode, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].target.project_id.as_deref(), Some(project.id.as_str()));
    }

    #[test]
    fn cross_entry_rebuild_reads_camel_case_project_id() {
        use crate::search::extractor::extract_starmap_entries;
        let dir = TempDir::new().unwrap();
        let starmaps_root = dir.path().join("app-meta").join("starmaps");
        let starmap_dir = starmaps_root.join("sm1");
        std::fs::create_dir_all(&starmap_dir).unwrap();
        std::fs::write(
            starmap_dir.join("graph.json"),
            serde_json::json!({"title": "CamelMap"}).to_string(),
        ).unwrap();
        std::fs::write(
            starmaps_root.join("sm1.meta.json"),
            serde_json::json!({"starmapId": "sm1", "projectId": "p1", "title": "CamelMap", "createdAt": 0, "updatedAt": 0}).to_string(),
        ).unwrap();

        let entries = extract_starmap_entries(dir.path(), None).unwrap();
        let title_entries: Vec<_> = entries.iter().filter(|e| e.scope == SearchScope::StarmapTitle).collect();
        assert_eq!(title_entries.len(), 1);
        assert_eq!(title_entries[0].target.project_id.as_deref(), Some("p1"));
    }

    #[test]
    fn cross_entry_delete_project_cascades_to_volumes_chapters_starmaps() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("CascadeProject").unwrap();
        let volume = api.create_volume(&project.id, "CascadeVol").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "CascadeCh").unwrap();
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, "CascadeBody").unwrap();
        let meta = api.create_starmap("CascadeMap", "desc", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        assert!(!api.search_service_search("CascadeVol", SearchScope::VolumeTitle, 10, None).is_empty());
        assert!(!api.search_service_search("CascadeCh", SearchScope::ChapterTitle, 10, None).is_empty());
        assert!(!api.search_service_search("CascadeBody", SearchScope::ChapterBody, 10, None).is_empty());
        assert!(!api.search_service_search("CascadeMap", SearchScope::StarmapTitle, 10, None).is_empty());

        api.delete_project(&project.id).unwrap();

        assert!(api.search_service_search("CascadeProject", SearchScope::ProjectTitle, 10, None).is_empty());
        assert!(api.search_service_search("CascadeVol", SearchScope::VolumeTitle, 10, None).is_empty());
        assert!(api.search_service_search("CascadeCh", SearchScope::ChapterTitle, 10, None).is_empty());
        assert!(api.search_service_search("CascadeBody", SearchScope::ChapterBody, 10, None).is_empty());
        assert!(api.search_service_search("CascadeMap", SearchScope::StarmapTitle, 10, None).len() == 1);
        assert!(api.search_service_search("CascadeMap", SearchScope::StarmapTitle, 10, None)[0].target.project_id.is_none(),
            "starmap must become unbound after project deletion");
    }

    #[test]
    fn cross_entry_delete_volume_cascades_to_chapters() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let volume = api.create_volume(&project.id, "VolWithChapters").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "ChapterInVol").unwrap();
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, "BodyInVol").unwrap();

        assert!(!api.search_service_search("ChapterInVol", SearchScope::ChapterTitle, 10, None).is_empty());
        assert!(!api.search_service_search("BodyInVol", SearchScope::ChapterBody, 10, None).is_empty());

        api.delete_volume(&project.id, &volume.id).unwrap();

        assert!(api.search_service_search("VolWithChapters", SearchScope::VolumeTitle, 10, None).is_empty());
        assert!(api.search_service_search("ChapterInVol", SearchScope::ChapterTitle, 10, None).is_empty());
        assert!(api.search_service_search("BodyInVol", SearchScope::ChapterBody, 10, None).is_empty());
    }

    #[test]
    fn cross_entry_create_starmap_json_then_search() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let _json = api.create_starmap_json("JsonMap", "desc").unwrap();
        let results = api.search_service_search("JsonMap", SearchScope::StarmapTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "JsonMap");
    }

    #[test]
    fn cross_entry_save_starmap_graph_removes_old_node_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let meta = api.create_starmap("GraphMap", "desc", None).unwrap();

        let node_a = crate::api::types::StarMapNodeDto {
            id: "node-a".to_string(),
            title: "NodeA".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: Some("content a".to_string()),
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let node_b = crate::api::types::StarMapNodeDto {
            id: "node-b".to_string(),
            title: "NodeB".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: Some("content b".to_string()),
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };

        let old_graph = crate::api::types::StarMapGraphDto {
            schema_version: 1,
            id: "g1".to_string(),
            starmap_id: meta.starmap_id.clone(),
            title: "GraphMap".to_string(),
            nodes: vec![node_a, node_b],
            edges: vec![],
            embeds: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        api.save_starmap_graph(&meta.starmap_id, &old_graph).unwrap();
        assert!(!api.search_service_search("NodeA", SearchScope::StarmapNode, 10, None).is_empty());
        assert!(!api.search_service_search("NodeB", SearchScope::StarmapNode, 10, None).is_empty());

        let node_a_only = crate::api::types::StarMapNodeDto {
            id: "node-a".to_string(),
            title: "NodeA".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: Some("content a".to_string()),
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let new_graph = crate::api::types::StarMapGraphDto {
            schema_version: 1,
            id: "g1".to_string(),
            starmap_id: meta.starmap_id.clone(),
            title: "GraphMap".to_string(),
            nodes: vec![node_a_only],
            edges: vec![],
            embeds: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        api.save_starmap_graph(&meta.starmap_id, &new_graph).unwrap();

        assert!(!api.search_service_search("NodeA", SearchScope::StarmapNode, 10, None).is_empty());
        assert!(api.search_service_search("NodeB", SearchScope::StarmapNode, 10, None).is_empty());
    }

    #[test]
    fn cross_entry_rebuild_after_incremental_matches() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("RebuildP2").unwrap();
        let volume = api.create_volume(&project.id, "V1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "RebuildCh2").unwrap();
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, "RebuildContent2").unwrap();

        let incremental_title = api.search_service_search("RebuildCh2", SearchScope::ChapterTitle, 10, None);
        let incremental_body = api.search_service_search("RebuildContent2", SearchScope::ChapterBody, 10, None);
        assert_eq!(incremental_title.len(), 1);
        assert_eq!(incremental_body.len(), 1);

        api.search_service_rebuild(None).unwrap();

        let rebuild_title = api.search_service_search("RebuildCh2", SearchScope::ChapterTitle, 10, None);
        let rebuild_body = api.search_service_search("RebuildContent2", SearchScope::ChapterBody, 10, None);
        let rebuild_project = api.search_service_search("RebuildP2", SearchScope::ProjectTitle, 10, None);
        let rebuild_volume = api.search_service_search("V1", SearchScope::VolumeTitle, 10, None);
        assert_eq!(rebuild_title.len(), 1);
        assert_eq!(rebuild_body.len(), 1);
        assert_eq!(rebuild_project.len(), 1);
        assert_eq!(rebuild_volume.len(), 1);
        assert_eq!(rebuild_title[0].title, "RebuildCh2");
        assert_eq!(rebuild_body[0].title, "RebuildCh2");
    }

    #[test]
    fn cross_entry_rebuild_starmap_matches_incremental() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let meta = api.create_starmap("RebuildStarMap", "desc", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let node = crate::api::types::StarMapNodeDto {
            id: String::new(),
            title: "RebuildNode".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: Some("rebuild node summary".to_string()),
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let _ = api.add_starmap_node(&meta.starmap_id, node, 0.0, 0.0);

        let inc_starmap = api.search_service_search("RebuildStarMap", SearchScope::StarmapTitle, 10, None);
        let inc_node = api.search_service_search("RebuildNode", SearchScope::StarmapNode, 10, None);
        assert_eq!(inc_starmap.len(), 1);
        assert_eq!(inc_node.len(), 1);

        api.search_service_rebuild(None).unwrap();

        let rb_starmap = api.search_service_search("RebuildStarMap", SearchScope::StarmapTitle, 10, None);
        let rb_node = api.search_service_search("RebuildNode", SearchScope::StarmapNode, 10, None);
        assert_eq!(rb_starmap.len(), 1);
        assert_eq!(rb_node.len(), 1);

        assert_eq!(inc_starmap[0].target.project_id, rb_starmap[0].target.project_id, "starmap project_id mismatch after rebuild");
        assert_eq!(inc_node[0].target.project_id, rb_node[0].target.project_id, "node project_id mismatch after rebuild");
    }

    #[test]
    fn cross_entry_rename_chapter_syncs_body_note_title() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let volume = api.create_volume(&project.id, "V1").unwrap();
        let chapter = api.create_chapter(&project.id, &volume.id, "OldTitle").unwrap();
        api.save_chapter_content(&project.id, &volume.id, &chapter.id, "SomeBody").unwrap();
        api.update_chapter_note(&project.id, &volume.id, &chapter.id, "SomeNote").unwrap();

        let body_before = api.search_service_search("SomeBody", SearchScope::ChapterBody, 10, None);
        assert_eq!(body_before.len(), 1);
        assert_eq!(body_before[0].title, "OldTitle");

        api.rename_chapter(&project.id, &volume.id, &chapter.id, "NewTitle").unwrap();

        let title_after = api.search_service_search("NewTitle", SearchScope::ChapterTitle, 10, None);
        assert_eq!(title_after.len(), 1);
        let body_after = api.search_service_search("SomeBody", SearchScope::ChapterBody, 10, None);
        assert_eq!(body_after.len(), 1);
        assert_eq!(body_after[0].title, "NewTitle");
        let note_after = api.search_service_search("SomeNote", SearchScope::ChapterNote, 10, None);
        assert_eq!(note_after.len(), 1);
        assert_eq!(note_after[0].title, "NewTitle");
    }

    #[test]
    fn cross_entry_bind_starmap_updates_child_project_id() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let meta = api.create_starmap("UnboundMap", "desc", None).unwrap();

        let node = crate::api::types::StarMapNodeDto {
            id: String::new(),
            title: "MapNode".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: Some("node text".to_string()),
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let _ = api.add_starmap_node(&meta.starmap_id, node, 0.0, 0.0);

        let node_before = api.search_service_search("MapNode", SearchScope::StarmapNode, 10, None);
        assert_eq!(node_before.len(), 1);
        assert!(node_before[0].target.project_id.is_none());

        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let node_after = api.search_service_search("MapNode", SearchScope::StarmapNode, 10, None);
        assert_eq!(node_after.len(), 1);
        assert_eq!(node_after[0].target.project_id.as_deref(), Some(project.id.as_str()));
    }

    #[test]
    fn cross_entry_unbind_starmap_clears_child_project_id() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let meta = api.create_starmap("BoundMap", "desc", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let node = crate::api::types::StarMapNodeDto {
            id: String::new(),
            title: "BoundNode".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: Some("node text".to_string()),
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let _ = api.add_starmap_node(&meta.starmap_id, node, 0.0, 0.0);

        let node_before = api.search_service_search("BoundNode", SearchScope::StarmapNode, 10, None);
        assert_eq!(node_before.len(), 1);
        assert_eq!(node_before[0].target.project_id.as_deref(), Some(project.id.as_str()));

        api.unbind_starmap_from_project(&meta.starmap_id).unwrap();

        let node_after = api.search_service_search("BoundNode", SearchScope::StarmapNode, 10, None);
        assert_eq!(node_after.len(), 1);
        assert!(node_after[0].target.project_id.is_none());
    }

    #[test]
    fn cross_entry_rebuild_hyperlink_uses_target_uri() {
        use crate::search::extractor::extract_starmap_entries;
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let meta = api.create_starmap("HLMap", "desc", None).unwrap();

        let hl = crate::starmap::types::StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: crate::starmap::types::StarMapEndpointPath {
                segments: vec![],
                endpoint: crate::starmap::types::StarMapEdgeEndpoint::Starmap,
            },
            target_uri: "https://example.com/docs".to_string(),
            label: Some("ExampleDoc".to_string()),
            target_starmap_id: None,
            created_at: 0,
            updated_at: 0,
        };
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join(&meta.starmap_id).join("hyperlinks");
        std::fs::create_dir_all(&starmap_dir).unwrap();
        std::fs::write(starmap_dir.join("hl1.json"), serde_json::to_string(&hl).unwrap()).unwrap();

        let entries = extract_starmap_entries(dir.path(), None).unwrap();
        let hl_entries: Vec<_> = entries.iter().filter(|e| e.scope == SearchScope::StarmapHyperlink).collect();
        assert_eq!(hl_entries.len(), 1);
        assert_eq!(hl_entries[0].title, "ExampleDoc");
        assert!(hl_entries[0].body.contains("example.com"));
    }

    #[test]
    fn cross_entry_save_starmap_graph_removes_old_edge_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let meta = api.create_starmap("EdgeMap", "desc", None).unwrap();

        let node_a = crate::api::types::StarMapNodeDto {
            id: "na".to_string(),
            title: "NodeA".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: None,
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let node_b = crate::api::types::StarMapNodeDto {
            id: "nb".to_string(),
            title: "NodeB".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: None,
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let edge_ab = crate::api::types::StarMapEdgeDto {
            id: "e1".to_string(),
            from: Some("na".to_string()),
            to: Some("nb".to_string()),
            kind: crate::api::types::StarMapEdgeKindDto::RelatedTo,
            label: Some("EdgeLabel".to_string()),
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };

        let old_graph = crate::api::types::StarMapGraphDto {
            schema_version: 1,
            id: "g1".to_string(),
            starmap_id: meta.starmap_id.clone(),
            title: "EdgeMap".to_string(),
            nodes: vec![node_a, node_b],
            edges: vec![edge_ab],
            embeds: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        api.save_starmap_graph(&meta.starmap_id, &old_graph).unwrap();
        assert!(!api.search_service_search("EdgeLabel", SearchScope::StarmapEdgeLabel, 10, None).is_empty());

        let node_a_only = crate::api::types::StarMapNodeDto {
            id: "na".to_string(),
            title: "NodeA".to_string(),
            kind: crate::api::types::StarMapNodeKindDto::Note,
            payload: None,
            tags: vec![],
            content: crate::api::types::StarMapNodeContentDto {
                kind: "inline".to_string(),
                summary: None,
                body: None,
                ..Default::default()
            },
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let new_graph = crate::api::types::StarMapGraphDto {
            schema_version: 1,
            id: "g1".to_string(),
            starmap_id: meta.starmap_id.clone(),
            title: "EdgeMap".to_string(),
            nodes: vec![node_a_only],
            edges: vec![],
            embeds: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };
        api.save_starmap_graph(&meta.starmap_id, &new_graph).unwrap();

        assert!(api.search_service_search("EdgeLabel", SearchScope::StarmapEdgeLabel, 10, None).is_empty());
        assert!(api.search_service_search("NodeB", SearchScope::StarmapNode, 10, None).is_empty());
    }

    #[test]
    fn create_project_indexes_default_volume() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let _project = api.create_project("VolIndexProject").unwrap();
        let results = api.search_service_search("第一卷", SearchScope::VolumeTitle, 10, None);
        assert_eq!(results.len(), 1, "create_project should index the auto-created default volume");
    }

    #[test]
    fn delete_project_preserves_unbound_starmap_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let _project = api.create_project("P1").unwrap();
        let unbound_meta = api.create_starmap("UnboundMap", "desc", None).unwrap();
        assert!(unbound_meta.project_id.is_none());

        let before = api.search_service_search("UnboundMap", SearchScope::StarmapTitle, 10, None);
        assert_eq!(before.len(), 1);

        api.delete_project(&_project.id).unwrap();

        let after = api.search_service_search("UnboundMap", SearchScope::StarmapTitle, 10, None);
        assert_eq!(after.len(), 1, "delete_project must not remove unbound starmap indices");
    }

    #[test]
    fn delete_project_determines_starmap_list_before_deletion() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("OrderP").unwrap();
        let meta = api.create_starmap("BoundMap", "desc", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let before = api.search_service_search("BoundMap", SearchScope::StarmapTitle, 10, None);
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].target.project_id.as_deref(), Some(project.id.as_str()));

        api.delete_project(&project.id).unwrap();

        let after = api.search_service_search("BoundMap", SearchScope::StarmapTitle, 10, None);
        assert_eq!(after.len(), 1,
            "bound starmap must remain searchable as unbound after project deletion");
        assert!(after[0].target.project_id.is_none(),
            "bound starmap project_id must be cleared after project deletion");
    }

    #[test]
    fn delete_project_unbinds_starmap_and_updates_child_indices() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("ProjWithMap").unwrap();
        let meta = api.create_starmap("MapWithNodes", "desc", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let node = crate::starmap::types::StarMapNode {
            id: "n1".to_string(),
            title: "NodeTitle".to_string(),
            kind: crate::starmap::types::StarMapNodeKind::Character,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        api.add_starmap_node(&meta.starmap_id, node.into(), 0.0, 0.0).unwrap();

        let before_node = api.search_service_search("NodeTitle", SearchScope::StarmapNode, 10, None);
        assert_eq!(before_node.len(), 1);
        assert_eq!(before_node[0].target.project_id.as_deref(), Some(project.id.as_str()));

        api.delete_project(&project.id).unwrap();

        let after_title = api.search_service_search("MapWithNodes", SearchScope::StarmapTitle, 10, None);
        assert_eq!(after_title.len(), 1);
        assert!(after_title[0].target.project_id.is_none());

        let after_node = api.search_service_search("NodeTitle", SearchScope::StarmapNode, 10, None);
        assert_eq!(after_node.len(), 1,
            "node index must remain after project deletion with project_id cleared");
        assert!(after_node[0].target.project_id.is_none(),
            "node project_id must be None after project deletion");

        let meta_after = api.core().get_starmap(&meta.starmap_id).unwrap();
        assert!(meta_after.project_id.is_none(),
            "starmap meta project_id must be cleared on disk after project deletion");
    }

    #[test]
    fn bind_unbind_updates_embed_and_hyperlink_project_id() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("P1").unwrap();
        let meta = api.create_starmap("EmbedHLMap", "desc", None).unwrap();

        let embed = crate::api::types::StarMapEmbedDto {
            instance_id: String::new(),
            target_starmap_id: "sm_child".to_string(),
            label: Some("MyEmbed".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: crate::api::types::StarMapEmbedPlacementDto {
                x: 0.0, y: 0.0, width: 100.0, height: 100.0, scale: 1.0, z_index: 0, collapsed: false,
            },
            target_viewport: crate::api::types::StarMapEmbedViewportDto {
                scale: 1.0, offset_x: 0.0, offset_y: 0.0,
            },
            source_node_id: None,
            host_endpoint: None,
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let _ = api.add_starmap_embed(&meta.starmap_id, embed);

        let hl = crate::starmap::types::StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: crate::starmap::types::StarMapEndpointPath {
                segments: vec![],
                endpoint: crate::starmap::types::StarMapEdgeEndpoint::Starmap,
            },
            target_uri: "https://example.com".to_string(),
            label: Some("MyHL".to_string()),
            target_starmap_id: None,
            created_at: 0,
            updated_at: 0,
        };
        {
            let mut store = crate::starmap::store::StarMapStore::new(dir.path(), &meta.starmap_id);
            store.load_full().unwrap();
            store.upsert_hyperlink(hl);
            store.flush().unwrap();
        }

        let embed_before = api.search_service_search("MyEmbed", SearchScope::StarmapEmbed, 10, None);
        assert_eq!(embed_before.len(), 1);
        assert!(embed_before[0].target.project_id.is_none());

        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let embed_after_bind = api.search_service_search("MyEmbed", SearchScope::StarmapEmbed, 10, None);
        assert_eq!(embed_after_bind.len(), 1);
        assert_eq!(embed_after_bind[0].target.project_id.as_deref(), Some(project.id.as_str()));

        let hl_after_bind = api.search_service_search("MyHL", SearchScope::StarmapHyperlink, 10, None);
        assert_eq!(hl_after_bind.len(), 1, "hyperlink index should exist after bind");
        assert_eq!(hl_after_bind[0].target.project_id.as_deref(), Some(project.id.as_str()),
            "hyperlink project_id must match bound project after bind");

        api.unbind_starmap_from_project(&meta.starmap_id).unwrap();

        let embed_after_unbind = api.search_service_search("MyEmbed", SearchScope::StarmapEmbed, 10, None);
        assert_eq!(embed_after_unbind.len(), 1);
        assert!(embed_after_unbind[0].target.project_id.is_none());

        let hl_after_unbind = api.search_service_search("MyHL", SearchScope::StarmapHyperlink, 10, None);
        assert_eq!(hl_after_unbind.len(), 1, "hyperlink index should exist after unbind");
        assert!(hl_after_unbind[0].target.project_id.is_none(),
            "hyperlink project_id must be None after unbind");
    }

    #[test]
    fn project_rebuild_preserves_other_project_indices() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project_a = api.create_project("ProjectA").unwrap();
        let project_b = api.create_project("ProjectB").unwrap();

        let _volume_a = api.create_volume(&project_a.id, "VolA").unwrap();
        let _volume_b = api.create_volume(&project_b.id, "VolB").unwrap();

        let before_a = api.search_service_search("VolA", SearchScope::VolumeTitle, 10, None);
        let before_b = api.search_service_search("VolB", SearchScope::VolumeTitle, 10, None);
        assert_eq!(before_a.len(), 1);
        assert_eq!(before_b.len(), 1);

        api.search_service_rebuild(Some(&project_a.id)).unwrap();

        let after_a = api.search_service_search("VolA", SearchScope::VolumeTitle, 10, None);
        let after_b = api.search_service_search("VolB", SearchScope::VolumeTitle, 10, None);
        assert_eq!(after_a.len(), 1, "project A indices should remain after project A rebuild");
        assert_eq!(after_b.len(), 1, "project B indices must not be cleared by project A rebuild");
    }

    #[test]
    fn delete_project_cascades_to_embed_and_hyperlink_indices() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project = api.create_project("CascadeP").unwrap();
        let meta = api.create_starmap("CascadeMap", "desc", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project.id).unwrap();

        let embed = crate::api::types::StarMapEmbedDto {
            instance_id: String::new(),
            target_starmap_id: "sm_child".to_string(),
            label: Some("CascadeEmbed".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: crate::api::types::StarMapEmbedPlacementDto {
                x: 0.0, y: 0.0, width: 100.0, height: 100.0, scale: 1.0, z_index: 0, collapsed: false,
            },
            target_viewport: crate::api::types::StarMapEmbedViewportDto {
                scale: 1.0, offset_x: 0.0, offset_y: 0.0,
            },
            source_node_id: None,
            host_endpoint: None,
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        let _ = api.add_starmap_embed(&meta.starmap_id, embed);

        let hl = crate::starmap::types::StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: crate::starmap::types::StarMapEndpointPath {
                segments: vec![],
                endpoint: crate::starmap::types::StarMapEdgeEndpoint::Starmap,
            },
            target_uri: "https://example.com".to_string(),
            label: Some("CascadeHL".to_string()),
            target_starmap_id: None,
            created_at: 0,
            updated_at: 0,
        };
        {
            let mut store = crate::starmap::store::StarMapStore::new(dir.path(), &meta.starmap_id);
            store.load_full().unwrap();
            store.upsert_hyperlink(hl);
            store.flush().unwrap();
        }

        api.search_service_rebuild(None).unwrap();

        let embed_before = api.search_service_search("CascadeEmbed", SearchScope::StarmapEmbed, 10, None);
        assert_eq!(embed_before.len(), 1, "embed index should exist before delete");
        let hl_before = api.search_service_search("CascadeHL", SearchScope::StarmapHyperlink, 10, None);
        assert_eq!(hl_before.len(), 1, "hyperlink index should exist before delete");

        api.delete_project(&project.id).unwrap();

        let embed_after = api.search_service_search("CascadeEmbed", SearchScope::StarmapEmbed, 10, None);
        assert_eq!(embed_after.len(), 1, "embed index must remain as unbound after project deletion");
        assert!(embed_after[0].target.project_id.is_none(), "embed project_id must be cleared after project deletion");
        let hl_after = api.search_service_search("CascadeHL", SearchScope::StarmapHyperlink, 10, None);
        assert_eq!(hl_after.len(), 1, "hyperlink index must remain as unbound after project deletion");
        assert!(hl_after[0].target.project_id.is_none(), "hyperlink project_id must be cleared after project deletion");
    }

    #[test]
    fn rebuild_extracts_starmap_embeds() {
        use crate::search::extractor::extract_starmap_entries;
        let dir = TempDir::new().unwrap();
        let starmaps_root = dir.path().join("app-meta").join("starmaps");
        let starmap_dir = starmaps_root.join("sm1");
        std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
        std::fs::write(
            starmap_dir.join("graph.json"),
            serde_json::json!({"title": "EmbedMap"}).to_string(),
        ).unwrap();
        std::fs::write(
            starmaps_root.join("sm1.meta.json"),
            serde_json::json!({"starmapId": "sm1", "projectId": "p1", "title": "EmbedMap", "createdAt": 0, "updatedAt": 0}).to_string(),
        ).unwrap();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: "em1".to_string(),
            target_starmap_id: "sm2".to_string(),
            label: Some("EmbedLabel".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: None,
            host_endpoint: None,
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        std::fs::write(
            starmap_dir.join("child_starmaps").join("em1.json"),
            serde_json::to_string(&embed).unwrap(),
        ).unwrap();

        let entries = extract_starmap_entries(dir.path(), None).unwrap();
        let embed_entries: Vec<_> = entries.iter().filter(|e| e.scope == SearchScope::StarmapEmbed).collect();
        assert_eq!(embed_entries.len(), 1, "rebuild should extract starmap embeds");
        assert_eq!(embed_entries[0].title, "EmbedLabel");
        assert_eq!(embed_entries[0].target.project_id.as_deref(), Some("p1"));
    }

    #[test]
    fn remove_by_target_project_id_removes_only_matching_entries() {
        let mut service = SearchIndexService::new();
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "project:p1".to_string(),
            scope: SearchScope::ProjectTitle,
            title: "P1".to_string(),
            body: "P1".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: None,
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "project:p2".to_string(),
            scope: SearchScope::ProjectTitle,
            title: "P2".to_string(),
            body: "P2".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p2".to_string()),
                volume_id: None,
                chapter_id: None,
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });
        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap:s1".to_string(),
            scope: SearchScope::StarmapTitle,
            title: "S1".to_string(),
            body: "S1".to_string(),
            target: Some(SearchTarget {
                project_id: None,
                volume_id: None,
                chapter_id: None,
                starmap_id: Some("s1".to_string()),
                node_id: None,
                setting_key: None,
            }),
        });
        assert_eq!(service.backend().entry_count(), 3);
        service.remove_by_prefix("___never_match___");
        service.backend_mut().remove_by_target_project_id("p1");
        let results = service.search("P1", SearchScope::All, 10, None);
        assert!(results.is_empty(), "p1 entry should be removed");
        let results_p2 = service.search("P2", SearchScope::All, 10, None);
        assert_eq!(results_p2.len(), 1, "p2 entry should remain");
        let results_s1 = service.search("S1", SearchScope::All, 10, None);
        assert_eq!(results_s1.len(), 1, "unbound starmap entry should remain");
    }

    #[test]
    fn project_rebuild_removes_stale_starmap_indices_from_previously_bound() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let project_a = api.create_project("ProjectA").unwrap();

        let meta = api.create_starmap("StaleMap", "", None).unwrap();
        api.bind_starmap_to_project(&meta.starmap_id, &project_a.id).unwrap();

        let node_before = api.search_service_search("StaleMap", SearchScope::StarmapTitle, 10, None);
        assert_eq!(node_before.len(), 1);
        assert_eq!(node_before[0].target.project_id.as_deref(), Some(project_a.id.as_str()));

        api.core().unbind_starmap_from_project(&meta.starmap_id).unwrap();

        api.search_service_rebuild(Some(&project_a.id)).unwrap();

        let stale_after = api.search_service_search("StaleMap", SearchScope::StarmapTitle, 10, None);
        assert!(stale_after.is_empty(),
            "stale starmap index with old project_id must be removed by per-project rebuild");
    }

    #[test]
    fn update_queue_clear_by_project_id_preserves_other_projects() {
        use crate::search::update_queue::SearchUpdateQueue;
        let mut queue = SearchUpdateQueue::new();
        queue.enqueue(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "chapter_body:p1:c1".to_string(),
            scope: SearchScope::ChapterBody,
            title: "P1 Chapter".to_string(),
            body: "content".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });
        queue.enqueue(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "chapter_body:p2:c2".to_string(),
            scope: SearchScope::ChapterBody,
            title: "P2 Chapter".to_string(),
            body: "content".to_string(),
            target: Some(SearchTarget {
                project_id: Some("p2".to_string()),
                volume_id: None,
                chapter_id: Some("c2".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            }),
        });
        queue.enqueue(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: "starmap:s1".to_string(),
            scope: SearchScope::StarmapTitle,
            title: "Unbound Map".to_string(),
            body: String::new(),
            target: Some(SearchTarget {
                project_id: None,
                volume_id: None,
                chapter_id: None,
                starmap_id: Some("s1".to_string()),
                node_id: None,
                setting_key: None,
            }),
        });
        assert_eq!(queue.len(), 3);
        queue.clear_by_project_id("p1");
        assert_eq!(queue.len(), 2, "only p1 entry should be removed");
        let remaining: Vec<_> = queue.drain();
        let ids: Vec<_> = remaining.iter().map(|u| u.object_id.as_str()).collect();
        assert!(ids.contains(&"chapter_body:p2:c2"), "p2 entry should remain");
        assert!(ids.contains(&"starmap:s1"), "unbound starmap entry should remain");
    }

    #[test]
    fn starmap_embed_scope_mapping_in_api() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let api = crate::api::service::WriterCoreApi::new(dir.path());
        let result = api.global_search_json("test", "starmapEmbed", 10, None);
        assert!(result.is_ok(), "starmapEmbed scope should be recognized");
    }
}
