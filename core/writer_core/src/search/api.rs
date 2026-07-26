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

    fn search_with_service(service: &SearchIndexService, query: &str, scope: SearchScope, limit: usize, cursor: Option<&str>) -> Vec<SearchResult> {
        service.search(query, scope, limit, cursor)
    }

    #[test]
    fn search_empty_query_returns_nothing() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_index(dir.path(), None);
        let service = SearchIndexService::new();
        let results = search_with_service(&service, "", SearchScope::All, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_nonexistent_query_returns_nothing() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_index(dir.path(), None);
        let service = SearchIndexService::new();
        let results = search_with_service(&service, "nonexistent", SearchScope::All, 10, None);
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
        service.process_updates();

        let results = search_with_service(&service, "Test", SearchScope::ChapterTitle, 10, None);
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
        service.process_updates();

        let results = search_with_service(&service, "Delete", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);

        service.enqueue_update(SearchIndexUpdate {
            action: SearchIndexAction::Delete,
            object_id: "del:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        service.process_updates();

        let results = search_with_service(&service, "Delete", SearchScope::ChapterTitle, 10, None);
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
        service.process_updates();

        let results = search_with_service(&service, "中文", SearchScope::ChapterBody, 10, None);
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
        service.process_updates();

        let page1 = search_with_service(&service, "Alpha", SearchScope::ChapterTitle, 2, None);
        assert_eq!(page1.len(), 2);

        let cursor = page1.last().unwrap().object_id.clone();
        let page2 = search_with_service(&service, "Alpha", SearchScope::ChapterTitle, 2, Some(&cursor));
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
            service.process_updates();
        }

        let results = search_with_service(&service, "Duplicate", SearchScope::ChapterTitle, 10, None);
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
        service.process_updates();

        let title_results = search_with_service(&service, "ScopeTest", SearchScope::ChapterTitle, 10, None);
        assert_eq!(title_results.len(), 1);
        let body_results = search_with_service(&service, "ScopeTest", SearchScope::ChapterBody, 10, None);
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
        service.process_updates();

        let title_results_after = search_with_service(&service, "ScopeTest", SearchScope::ChapterTitle, 10, None);
        assert!(title_results_after.is_empty());
        let body_results_after = search_with_service(&service, "ScopeTest", SearchScope::ChapterBody, 10, None);
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
        service.process_updates();

        let results = search_with_service(&service, "ChapterRef", SearchScope::StarmapLink, 10, None);
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
}
