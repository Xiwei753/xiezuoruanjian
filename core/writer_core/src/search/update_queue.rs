use std::collections::VecDeque;

use super::types::{SearchIndexAction, SearchIndexUpdate};

pub struct SearchUpdateQueue {
    queue: VecDeque<SearchIndexUpdate>,
    pending_object_ids: std::collections::HashSet<String>,
}

impl SearchUpdateQueue {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            pending_object_ids: std::collections::HashSet::new(),
        }
    }

    pub fn enqueue(&mut self, update: SearchIndexUpdate) {
        if self.pending_object_ids.contains(&update.object_id) {
            if let Some(existing) = self.queue.iter_mut().find(|u| u.object_id == update.object_id) {
                let should_remove = matches!(update.action, SearchIndexAction::Delete)
                    && matches!(existing.action, SearchIndexAction::Upsert);
                *existing = update;
                if should_remove {
                    self.pending_object_ids.remove(&existing.object_id);
                }
                return;
            }
        }
        let is_delete = matches!(update.action, SearchIndexAction::Delete);
        self.pending_object_ids.insert(update.object_id.clone());
        self.queue.push_back(update);
        if is_delete {
            self.pending_object_ids.remove(
                self.queue.back().map(|u| &u.object_id).unwrap_or(&String::new()),
            );
        }
    }

    pub fn drain(&mut self) -> Vec<SearchIndexUpdate> {
        self.pending_object_ids.clear();
        self.queue.drain(..).collect()
    }

    pub fn clear(&mut self) {
        self.pending_object_ids.clear();
        self.queue.clear();
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl Default for SearchUpdateQueue {
    fn default() -> Self {
        Self::new()
    }
}
