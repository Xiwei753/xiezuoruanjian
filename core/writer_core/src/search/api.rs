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
            object_id: "test:1".to_string(),
            scope: SearchScope::ChapterTitle,
            title: "Test Chapter".to_string(),
            body: "Test Chapter".to_string(),
            target: SearchTarget {
                project_id: Some("p1".to_string()),
                volume_id: None,
                chapter_id: Some("c1".to_string()),
                starmap_id: None,
                node_id: None,
                setting_key: None,
            },
        });
        process_pending_updates();

        let results = global_search("Test", SearchScope::ChapterTitle, 10, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Test Chapter");
    }
}
