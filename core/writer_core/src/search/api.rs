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
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("sm1");
        std::fs::create_dir_all(starmap_dir.join("links")).unwrap();
        std::fs::write(
            starmap_dir.join("graph.json"),
            serde_json::json!({"title": "TestMap", "linkIds": []}).to_string(),
        ).unwrap();
        std::fs::write(
            starmap_dir.join("sm1.meta.json"),
            serde_json::json!({"projectId": "p1"}).to_string(),
        ).unwrap();
        std::fs::write(
            starmap_dir.join("links").join("l1.json"),
            serde_json::json!({"linkId": "l1", "label": "MyLink"}).to_string(),
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
            scope: SearchScope::StarmapNode,
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
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("sm1");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::write(
            starmap_dir.join("graph.json"),
            serde_json::json!({"title": "TestMap"}).to_string(),
        ).unwrap();
        std::fs::write(
            starmap_dir.join("sm1.meta.json"),
            serde_json::json!({"projectId": "p1"}).to_string(),
        ).unwrap();
        std::fs::write(
            starmap_dir.join("nodes").join("n1.json"),
            serde_json::json!({
                "id": "n1",
                "title": "MyNode",
                "content": {"type": "inline", "summary": "节点摘要", "body": "节点正文"},
                "tags": ["标签A", "标签B"]
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
}
