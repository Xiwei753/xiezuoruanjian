use super::backend::SearchBackend;
use super::extractor::{ExtractionFailure, ExtractionReport};
use super::types::*;
use super::update_queue::SearchUpdateQueue;

pub struct SearchIndexService {
    backend: SearchBackend,
    update_queue: SearchUpdateQueue,
    last_rebuild_at: u64,
    is_rebuilding: bool,
    /// 上一次重建中失败的来源（保留旧索引的那些来源）。
    last_rebuild_failures: Vec<ExtractionFailure>,
}

impl SearchIndexService {
    pub fn new() -> Self {
        Self {
            backend: SearchBackend::new(),
            update_queue: SearchUpdateQueue::new(),
            last_rebuild_at: 0,
            is_rebuilding: false,
            last_rebuild_failures: Vec::new(),
        }
    }

    pub fn search(
        &mut self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
    ) -> Vec<SearchResult> {
        self.search_page(query, scope, limit, cursor, &SearchContext::default())
            .items
    }

    pub fn search_page(
        &mut self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
        context: &SearchContext,
    ) -> SearchResultPage {
        self.apply_queue();
        self.backend.search(query, scope, limit, cursor, context)
    }

    /// 在授权范围内搜索，并在分页前裁剪候选集。
    pub fn search_page_authorized(
        &mut self,
        query: &str,
        scope: SearchScope,
        limit: usize,
        cursor: Option<&str>,
        context: &SearchContext,
        authorization: &SearchAuthorization,
    ) -> SearchResultPage {
        self.apply_queue();
        self.backend
            .search_authorized(query, scope, limit, cursor, context, authorization)
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
        self.enqueue_updates(vec![update]);
    }

    /// 批量发布已随对象事务提交的索引更新，仅触发一次队列应用。
    pub fn enqueue_updates(&mut self, updates: Vec<SearchIndexUpdate>) {
        if updates.is_empty() {
            return;
        }
        for update in updates {
            self.update_queue.enqueue(update);
        }
        self.apply_queue();
    }

    /// 按来源事务中的原始顺序发布搜索动作。
    ///
    /// 前缀删除前先应用已入队更新，再直接删除后端条目，保证后续动作仍按
    /// 事务记录的顺序生效；全部处理完后再统一应用一次队列。
    pub fn apply_transaction_actions(&mut self, actions: Vec<SearchIndexTransactionAction>) {
        if actions.is_empty() {
            return;
        }
        for action in actions {
            match action {
                SearchIndexTransactionAction::Update(update) => {
                    self.update_queue.enqueue(update);
                }
                SearchIndexTransactionAction::DeletePrefix(prefix) => {
                    self.apply_queue();
                    self.backend.remove_by_prefix(&prefix);
                }
            }
        }
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
                        target: update.target.unwrap_or_default(),
                    });
                }
            }
        }
    }

    pub fn remove_by_prefix(&mut self, prefix: &str) {
        self.backend.remove_by_prefix(prefix);
    }

    /// 用一次完整提取结果替换全量索引。
    ///
    /// 完整提取（没有失败来源）时用新条目一次性替换整个索引；部分来源失败时
    /// 保留这些来源上一次有效的条目，其余来源按本次提取结果更新。不会先清空
    /// 索引，也不会丢弃重建期间已入队的合法增量更新。
    pub fn rebuild_from_extraction(&mut self, report: ExtractionReport) {
        self.apply_rebuild(None, report);
    }

    /// 用一次按作品提取结果替换该作品的索引条目。
    ///
    /// 与全量重建走同一套替换语义：其它作品的条目、以及失败来源的旧条目原样
    /// 保留；该作品成功提取的来源按本次结果更新。
    pub fn rebuild_project_from_extraction(&mut self, project_id: &str, report: ExtractionReport) {
        self.apply_rebuild(Some(project_id), report);
    }

    /// 上一次重建中保留旧索引的失败来源。
    pub fn last_rebuild_failures(&self) -> &[ExtractionFailure] {
        &self.last_rebuild_failures
    }

    fn apply_rebuild(&mut self, project_id: Option<&str>, report: ExtractionReport) {
        self.is_rebuilding = true;
        let mut entries = Vec::new();
        let mut protected_counts = vec![0usize; report.failures.len()];

        for entry in self.backend.entries_snapshot() {
            let in_scope = project_id
                .map_or(true, |pid| entry.target.project_id.as_deref() == Some(pid));
            if !in_scope {
                entries.push(entry);
                continue;
            }
            if let Some(index) = report
                .failures
                .iter()
                .position(|failure| failure.protects(&entry))
            {
                protected_counts[index] += 1;
                entries.push(entry);
            }
        }

        for (failure, protected) in report.failures.iter().zip(protected_counts) {
            if protected == 0 {
                log::warn!(
                    "search index rebuild: source {} failed ({}); no previous entries retained for scope {:?}",
                    failure.path.display(),
                    failure.error,
                    failure.scope
                );
            } else {
                log::warn!(
                    "search index rebuild: source {} failed ({}); retained {protected} previous entries for scope {:?}",
                    failure.path.display(),
                    failure.error,
                    failure.scope
                );
            }
        }

        entries.extend(report.entries);
        self.backend.replace_entries(entries);
        // 重建前已经入队的增量更新属于更新的事实，替换后再应用，不能清空。
        self.apply_queue();
        self.last_rebuild_failures = report.failures;
        self.last_rebuild_at = super::extractor::now_epoch();
        self.is_rebuilding = false;
    }

    pub fn backend(&self) -> &SearchBackend {
        &self.backend
    }

    pub fn annotation_object_ids_for_project(&self, project_id: &str) -> Vec<String> {
        self.backend.annotation_object_ids_for_project(project_id)
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
