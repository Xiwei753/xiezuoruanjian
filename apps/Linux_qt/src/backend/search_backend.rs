// =============================================================================
// search_backend.rs — Linux Qt 全局搜索后端
// =============================================================================
//
// 搜索索引只属于当前工作区的 Core 实例。Qt 端普通 getter 每次都临时构造
// WriterCoreApi，临时实例里的索引为空，不能用于搜索；因此这里把打开工作区时
// 构造的 Core API 交给一个常驻 worker 线程独占：
//   - 打开工作区后重建一次索引；
//   - 之后的搜索都在该索引上执行，按键查询不再读盘；
//   - 保存/结构/设置变化提交增量索引更新，只有外部数据替换才整体重建；
//   - QML 只拿结果 JSON，不接触整章正文。
// =============================================================================

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use qmetaobject::prelude::*;
use writer_core::api::WriterCoreApi;
use writer_core::search::{SearchContext, SearchIndexUpdate};
use writer_core::storage::git_repo_layout::GitRepoLayout;

enum SearchCommand {
    Open {
        data_root: String,
        projects_root: String,
        layout: GitRepoLayout,
    },
    ApplyUpdates(Vec<SearchIndexUpdate>),
    RebuildProject(String),
    RebuildAll,
    Query {
        request_id: String,
        query: String,
        scope: String,
        limit: u32,
        cursor: Option<String>,
        context_json: String,
        append: bool,
        reply: Box<dyn FnOnce(SearchReply) + Send>,
    },
    Close,
}

pub(crate) struct SearchReply {
    request_id: String,
    page_json: String,
    append: bool,
    error: String,
}

/// UI 线程与搜索 worker 之间的唯一通道。
pub(crate) struct SearchBridge {
    tx: Mutex<Option<Sender<SearchCommand>>>,
}

impl Default for SearchBridge {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let spawn_result = std::thread::Builder::new()
            .name("linux-qt-global-search".to_string())
            .spawn(move || search_worker_loop(rx));
        if let Err(error) = spawn_result {
            log::error!("failed to start global search worker: {error}");
        }
        Self {
            tx: Mutex::new(Some(tx)),
        }
    }
}

impl SearchBridge {
    fn send(&self, command: SearchCommand) -> bool {
        match self.tx.lock() {
            Ok(guard) => guard
                .as_ref()
                .map(|tx| tx.send(command).is_ok())
                .unwrap_or(false),
            Err(poisoned) => poisoned
                .into_inner()
                .as_ref()
                .map(|tx| tx.send(command).is_ok())
                .unwrap_or(false),
        }
    }

    pub(crate) fn open(&self, data_root: String, projects_root: String, layout: GitRepoLayout) {
        let _ = self.send(SearchCommand::Open {
            data_root,
            projects_root,
            layout,
        });
    }

    pub(crate) fn apply_updates(&self, updates: Vec<SearchIndexUpdate>) {
        if !updates.is_empty() {
            let _ = self.send(SearchCommand::ApplyUpdates(updates));
        }
    }

    pub(crate) fn rebuild_project(&self, project_id: String) {
        let _ = self.send(SearchCommand::RebuildProject(project_id));
    }

    pub(crate) fn rebuild_all(&self) {
        let _ = self.send(SearchCommand::RebuildAll);
    }

    pub(crate) fn close(&self) {
        let _ = self.send(SearchCommand::Close);
    }

    fn query(
        &self,
        request_id: String,
        query: String,
        scope: String,
        limit: u32,
        cursor: Option<String>,
        context_json: String,
        append: bool,
        reply: Box<dyn FnOnce(SearchReply) + Send>,
    ) {
        let _ = self.send(SearchCommand::Query {
            request_id,
            query,
            scope,
            limit,
            cursor,
            context_json,
            append,
            reply,
        });
    }
}

struct WorkerState {
    api: Option<WriterCoreApi>,
}

impl WorkerState {
    fn rebuild(&mut self) -> Result<(), String> {
        if let Some(api) = self.api.as_ref() {
            api.search_service_rebuild(None)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn apply_updates(&mut self, updates: Vec<SearchIndexUpdate>) {
        if let Some(api) = self.api.as_ref() {
            for update in updates {
                api.apply_search_index_update(update);
            }
        }
    }

    fn rebuild_project(&mut self, project_id: &str) {
        if let Some(api) = self.api.as_ref() {
            if let Err(error) = api.search_service_rebuild(Some(project_id)) {
                log::warn!("global search project rebuild failed for {project_id}: {error}");
            }
        }
    }

    fn query(
        &mut self,
        query: &str,
        scope: &str,
        limit: u32,
        cursor: Option<&str>,
        context_json: &str,
    ) -> Result<String, String> {
        let Some(api) = self.api.as_ref() else {
            return Err("search.index.not_ready".to_string());
        };
        let context = serde_json::from_str::<SearchContext>(context_json).unwrap_or_default();
        api.global_search_page_json(query, scope, limit, cursor, Some(&context))
            .map_err(|error| error.to_string())
    }
}

fn search_worker_loop(rx: Receiver<SearchCommand>) {
    let mut state = WorkerState { api: None };
    while let Ok(command) = rx.recv() {
        match command {
            SearchCommand::Open {
                data_root,
                projects_root,
                layout,
            } => {
                let api =
                    crate::backend::app_backend::with_layout_core_api(&data_root, &projects_root, &layout);
                state.api = Some(api);
                if let Err(error) = state.rebuild() {
                    log::warn!("global search index rebuild failed on open: {error}");
                }
            }
            SearchCommand::ApplyUpdates(updates) => state.apply_updates(updates),
            SearchCommand::RebuildProject(project_id) => {
                state.rebuild_project(&project_id);
            }
            SearchCommand::RebuildAll => {
                if let Err(error) = state.rebuild() {
                    log::warn!("global search index full rebuild failed: {error}");
                }
            }
            SearchCommand::Query {
                request_id,
                query,
                scope,
                limit,
                cursor,
                context_json,
                append,
                reply,
            } => {
                let result = state.query(
                    &query,
                    &scope,
                    limit,
                    cursor.as_deref(),
                    &context_json,
                );
                let (page_json, error) = match result {
                    Ok(page) => (page, String::new()),
                    Err(error) => (String::new(), error),
                };
                reply(SearchReply {
                    request_id,
                    page_json,
                    append,
                    error,
                });
            }
            SearchCommand::Close => {
                state.api = None;
            }
        }
    }
}

/// QML 可见的搜索后端。查询始终走 worker，不阻塞 UI 线程。
#[allow(non_snake_case)]
#[derive(QObject)]
pub struct SearchBackend {
    base: qt_base_class!(trait QObject),
    search: qt_method!(fn(
        &mut self,
        query: QString,
        scope: QString,
        limit: u32,
        cursor: QString,
        context_json: QString,
        append: bool
    ) -> QString),
    results_ready:
        qt_signal!(request_id: QString, page_json: QString, append: bool, error: QString),
    bridge: Arc<SearchBridge>,
    next_request_id: u64,
}

impl SearchBackend {
    pub(crate) fn new(bridge: Arc<SearchBridge>) -> Self {
        Self {
            base: Default::default(),
            search: Default::default(),
            results_ready: Default::default(),
            bridge,
            next_request_id: 0,
        }
    }

    fn search(
        &mut self,
        query: QString,
        scope: QString,
        limit: u32,
        cursor: QString,
        context_json: QString,
        append: bool,
    ) -> QString {
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let request_id = format!("search-{}", self.next_request_id);
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |reply: SearchReply| {
            if let Some(pinned) = qptr.as_pinned() {
                let this = pinned.borrow();
                let _ = this.results_ready(
                    reply.request_id.into(),
                    reply.page_json.into(),
                    reply.append,
                    reply.error.into(),
                );
            }
        });
        let cursor = cursor.to_string();
        self.bridge.query(
            request_id.clone(),
            query.to_string(),
            scope.to_string(),
            limit,
            if cursor.is_empty() {
                None
            } else {
                Some(cursor)
            },
            context_json.to_string(),
            append,
            Box::new(callback),
        );
        request_id.into()
    }
}
