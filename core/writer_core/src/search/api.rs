use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::error::Result;
use super::types::*;
use super::service::SearchIndexService;

static GLOBAL_SEARCH_SERVICE: OnceLock<Mutex<SearchIndexService>> = OnceLock::new();

fn get_or_init_service() -> &'static Mutex<SearchIndexService> {
    GLOBAL_SEARCH_SERVICE.get_or_init(|| Mutex::new(SearchIndexService::new()))
}

pub fn global_search(
    query: &str,
    scope: SearchScope,
    limit: usize,
    cursor: Option<&str>,
) -> Vec<SearchResult> {
    let service = get_or_init_service();
    let guard = service.lock().unwrap_or_else(|e| e.into_inner());
    guard.search(query, scope, limit, cursor)
}

pub fn rebuild_search_index(workspace: &Path, project_id: Option<&str>) -> Result<SearchIndexStatus> {
    let entries = super::rebuild::rebuild_index(workspace, project_id)?;
    let service = get_or_init_service();
    let mut guard = service.lock().unwrap_or_else(|e| e.into_inner());
    guard.rebuild_from_entries(entries);
    Ok(guard.status())
}

pub fn get_search_index_status() -> SearchIndexStatus {
    let service = get_or_init_service();
    let guard = service.lock().unwrap_or_else(|e| e.into_inner());
    guard.status()
}

pub fn enqueue_search_index_update(update: SearchIndexUpdate) {
    let service = get_or_init_service();
    let mut guard = service.lock().unwrap_or_else(|e| e.into_inner());
    guard.enqueue_update(update);
}

pub fn process_pending_updates() {
    let service = get_or_init_service();
    let mut guard = service.lock().unwrap_or_else(|e| e.into_inner());
    guard.process_updates();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn search_empty_query_returns_nothing() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);
        let results = global_search("", SearchScope::All, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_nonexistent_query_returns_nothing() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);
        let results = global_search("nonexistent", SearchScope::All, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_index_status_default() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);
        let status = get_search_index_status();
        assert_eq!(status.total_entries, 0);
    }

    #[test]
    fn enqueue_and_search_update() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);

        enqueue_search_index_update(SearchIndexUpdate {
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
        process_pending_updates();

        let results = global_search("Test", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test Chapter");
        assert_eq!(results[0].object_id, "test:1");
    }

    #[test]
    fn search_delete_removes_entry() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);

        enqueue_search_index_update(SearchIndexUpdate {
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
        process_pending_updates();

        let results = global_search("Delete", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);

        enqueue_search_index_update(SearchIndexUpdate {
            action: SearchIndexAction::Delete,
            object_id: "del:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        process_pending_updates();

        let results = global_search("Delete", SearchScope::ChapterTitle, 10, None);
        assert!(results.is_empty());
    }

    #[test]
    fn search_chinese_text_no_panic() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);

        enqueue_search_index_update(SearchIndexUpdate {
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
        process_pending_updates();

        let results = global_search("中文", SearchScope::ChapterBody, 10, None);
        assert!(!results.is_empty());
        assert!(results[0].summary.contains("中文"));
    }

    #[test]
    fn search_stable_pagination() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);

        for i in 0..5 {
            enqueue_search_index_update(SearchIndexUpdate {
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
        process_pending_updates();

        let page1 = global_search("Alpha", SearchScope::ChapterTitle, 2, None);
        assert_eq!(page1.len(), 2);

        let cursor = page1.last().unwrap().object_id.clone();
        let page2 = global_search("Alpha", SearchScope::ChapterTitle, 2, Some(&cursor));
        assert_eq!(page2.len(), 2);

        let all_ids: Vec<String> = page1.iter().chain(page2.iter()).map(|r| r.object_id.clone()).collect();
        let unique_ids: std::collections::HashSet<&String> = all_ids.iter().collect();
        assert_eq!(unique_ids.len(), 4);
    }

    #[test]
    fn search_reinsert_dedup_scope_index() {
        let dir = TempDir::new().unwrap();
        let _ = rebuild_search_index(dir.path(), None);

        for _ in 0..3 {
            enqueue_search_index_update(SearchIndexUpdate {
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
            process_pending_updates();
        }

        let results = global_search("Duplicate", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);
    }
}
