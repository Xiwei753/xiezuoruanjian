use std::collections::HashMap;

use super::types::*;

pub struct SearchBackend {
    entries: HashMap<String, IndexEntry>,
    scope_index: HashMap<SearchScope, Vec<String>>,
}

impl SearchBackend {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            scope_index: HashMap::new(),
        }
    }

    pub fn insert(&mut self, entry: IndexEntry) {
        let id = entry.object_id.clone();
        let scope = entry.scope;
        self.entries.insert(id.clone(), entry);
        self.scope_index.entry(scope).or_default().push(id);
    }

    pub fn remove(&mut self, object_id: &str) {
        if let Some(entry) = self.entries.remove(object_id) {
            if let Some(ids) = self.scope_index.get_mut(&entry.scope) {
                ids.retain(|id| id != object_id);
            }
        }
    }

    pub fn search(
        &self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
    ) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        let candidate_ids: Vec<&String> = if scope == SearchScope::All {
            self.entries.keys().collect()
        } else {
            self.scope_index.get(&scope).map(|ids| ids.iter().collect()).unwrap_or_default()
        };

        let mut past_cursor = cursor.is_none();
        for id in candidate_ids {
            if !past_cursor {
                if Some(id.as_str()) == cursor {
                    past_cursor = true;
                }
                continue;
            }

            if let Some(entry) = self.entries.get(id) {
                let title_match = entry.title.to_lowercase().contains(&query_lower);
                let body_match = entry.body.to_lowercase().contains(&query_lower);

                if title_match || body_match {
                    let mut match_ranges = Vec::new();
                    if title_match {
                        if let Some(pos) = entry.title.to_lowercase().find(&query_lower) {
                            match_ranges.push((pos, pos + query.len()));
                        }
                    }
                    if body_match {
                        if let Some(pos) = entry.body.to_lowercase().find(&query_lower) {
                            match_ranges.push((pos, pos + query.len()));
                        }
                    }

                    let score = if title_match { 2.0 } else { 1.0 };

                    let summary = if entry.body.len() > 200 {
                        if let Some(pos) = entry.body.to_lowercase().find(&query_lower) {
                            let start = pos.saturating_sub(50);
                            let end = (pos + query.len() + 150).min(entry.body.len());
                            format!("...{}...", &entry.body[start..end])
                        } else {
                            format!("{}...", &entry.body[..200])
                        }
                    } else {
                        entry.body.clone()
                    };

                    results.push(SearchResult {
                        title: entry.title.clone(),
                        path: format!("{:?}", entry.scope),
                        summary,
                        match_ranges,
                        score,
                        scope: entry.scope,
                        target: entry.target.clone(),
                    });

                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn scope_count(&self, scope: SearchScope) -> usize {
        self.scope_index.get(&scope).map(|ids| ids.len()).unwrap_or(0)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.scope_index.clear();
    }
}
