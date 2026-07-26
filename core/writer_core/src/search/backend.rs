use std::collections::HashMap;

use super::types::*;

fn find_char_boundary_safe(s: &str, byte_pos: usize) -> usize {
    if byte_pos >= s.len() {
        return s.len();
    }
    let mut pos = byte_pos;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

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
        if let Some(old_entry) = self.entries.insert(id.clone(), entry) {
            if old_entry.scope != scope {
                if let Some(ids) = self.scope_index.get_mut(&old_entry.scope) {
                    ids.retain(|i| i != &id);
                }
                self.scope_index.entry(scope).or_default().push(id);
            }
        } else {
            self.scope_index.entry(scope).or_default().push(id);
        }
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

        let mut candidate_ids: Vec<String> = if scope == SearchScope::All {
            self.entries.keys().cloned().collect()
        } else {
            self.scope_index.get(&scope).cloned().unwrap_or_default()
        };
        candidate_ids.sort();

        let mut all_results = Vec::new();

        for id in &candidate_ids {
            if let Some(entry) = self.entries.get(id) {
                let title_lower = entry.title.to_lowercase();
                let body_lower = entry.body.to_lowercase();

                let title_match = title_lower.contains(&query_lower);
                let body_match = body_lower.contains(&query_lower);

                if title_match || body_match {
                    let mut match_ranges = Vec::new();
                    if title_match {
                        if let Some(pos) = title_lower.find(&query_lower) {
                            let end_pos = pos + query_lower.len();
                            match_ranges.push((pos, end_pos));
                        }
                    }
                    if body_match {
                        if let Some(pos) = body_lower.find(&query_lower) {
                            let end_pos = pos + query_lower.len();
                            match_ranges.push((pos, end_pos));
                        }
                    }

                    let score = if title_match { 2.0 } else { 1.0 };

                    let summary = if entry.body.len() > 200 {
                        if let Some(pos) = body_lower.find(&query_lower) {
                            let start = pos.saturating_sub(50);
                            let end = (pos + query_lower.len() + 150).min(entry.body.len());
                            let start = find_char_boundary_safe(&entry.body, start);
                            let end = find_char_boundary_safe(&entry.body, end);
                            if start < end {
                                format!("...{}...", &entry.body[start..end])
                            } else {
                                String::new()
                            }
                        } else {
                            let end = find_char_boundary_safe(&entry.body, 200);
                            format!("{}...", &entry.body[..end])
                        }
                    } else {
                        entry.body.clone()
                    };

                    all_results.push(SearchResult {
                        title: entry.title.clone(),
                        path: format!("{:?}", entry.scope),
                        summary,
                        match_ranges,
                        score,
                        scope: entry.scope,
                        target: entry.target.clone(),
                        object_id: entry.object_id.clone(),
                    });
                }
            }
        }

        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.object_id.cmp(&b.object_id))
        });

        let skip = if let Some(cursor) = cursor {
            all_results
                .iter()
                .position(|r| r.object_id == cursor)
                .map(|p| p + 1)
                .unwrap_or(0)
        } else {
            0
        };

        all_results
            .into_iter()
            .skip(skip)
            .take(limit)
            .collect()
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
