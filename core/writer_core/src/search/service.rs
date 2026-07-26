use super::types::*;
use super::backend::SearchBackend;
use super::update_queue::SearchUpdateQueue;

pub struct SearchIndexService {
    backend: SearchBackend,
    update_queue: SearchUpdateQueue,
    last_rebuild_at: u64,
    is_rebuilding: bool,
}

impl SearchIndexService {
    pub fn new() -> Self {
        Self {
            backend: SearchBackend::new(),
            update_queue: SearchUpdateQueue::new(),
            last_rebuild_at: 0,
            is_rebuilding: false,
        }
    }

    pub fn search(
        &mut self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
    ) -> Vec<SearchResult> {
        self.apply_queue();
        self.backend.search(query, scope, limit, cursor)
    }

    pub fn status(&self) -> SearchIndexStatus {
        let scope_counts: Vec<(SearchScope, usize)> = SearchScope::all_scopes()
            .iter()
            .map(|s| (*s, self.backend.scope_count(*s)))
            .collect();

        SearchIndexStatus {
            total_entries: self.backend.entry_count(),
            scope_counts,
            last_rebuild_at: self.last_rebuild_at,
            is_rebuilding: self.is_rebuilding,
        }
    }

    pub fn enqueue_update(&mut self, update: SearchIndexUpdate) {
        self.update_queue.enqueue(update);
        self.apply_queue();
    }

    fn apply_queue(&mut self) {
        let updates = self.update_queue.drain();
        for update in updates {
            match update.action {
                SearchIndexAction::Delete => {
                    self.backend.remove(&update.object_id);
                }
                SearchIndexAction::Upsert => {
                    self.backend.insert(IndexEntry {
                        object_id: update.object_id,
                        scope: update.scope,
                        title: update.title,
                        body: update.body,
                        target: update.target.unwrap_or(SearchTarget {
                            project_id: None,
                            volume_id: None,
                            chapter_id: None,
                            starmap_id: None,
                            node_id: None,
                            setting_key: None,
                        }),
                    });
                }
            }
        }
    }

    pub fn remove_by_prefix(&mut self, prefix: &str) {
        self.backend.remove_by_prefix(prefix);
    }

    pub fn rebuild_from_entries(&mut self, entries: Vec<IndexEntry>) {
        self.is_rebuilding = true;
        self.backend.clear();
        self.update_queue.clear();
        for entry in entries {
            self.backend.insert(entry);
        }
        self.last_rebuild_at = super::extractor::now_epoch();
        self.is_rebuilding = false;
    }

    pub fn rebuild_project_from_entries(&mut self, project_id: &str, entries: Vec<IndexEntry>) {
        self.is_rebuilding = true;
        self.backend.remove_by_target_project_id(project_id);
        for entry in entries {
            self.backend.insert(entry);
        }
        self.update_queue.clear();
        self.last_rebuild_at = super::extractor::now_epoch();
        self.is_rebuilding = false;
    }

    pub fn backend(&self) -> &SearchBackend {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut SearchBackend {
        &mut self.backend
    }
}

impl Default for SearchIndexService {
    fn default() -> Self {
        Self::new()
    }
}
