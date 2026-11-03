//! Qt adapter for Core's shared tags, text anchors, named entries and links.
//!
//! All annotation state is owned by `WriterCoreApi`; this QObject only converts
//! JSON arguments/results for QML and never keeps a second copy of that state.

use qmetaobject::prelude::*;
use qmetaobject::QString;
use serde::{de::DeserializeOwned, Serialize};
use writer_core::api::types::{
    AnnotationEditDto, AnnotationObjectRefDto, AnnotationTargetDto, EditorTransactionCauseDto,
};
use writer_core::api::{ResultEnvelope, WriterCoreApi, WriterError};

use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, OnceLock};

use writer_core::storage::git_repo_layout::GitRepoLayout;

/// 标注后台任务所属工作区。AppRef / AppBackend 仍只在 GUI 线程使用。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AnnotationContext {
    data_root: String,
    projects_root: String,
    worktree_root: String,
    git_dir: String,
}

impl AnnotationContext {
    fn new(data_root: &str, projects_root: &str, layout: &GitRepoLayout) -> Self {
        Self {
            data_root: data_root.to_string(),
            projects_root: projects_root.to_string(),
            worktree_root: layout.worktree_root.to_string_lossy().into_owned(),
            git_dir: layout.git_dir.to_string_lossy().into_owned(),
        }
    }

    fn key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.data_root, self.projects_root, self.worktree_root, self.git_dir
        )
    }

    fn layout(&self) -> GitRepoLayout {
        GitRepoLayout {
            worktree_root: std::path::PathBuf::from(&self.worktree_root),
            git_dir: std::path::PathBuf::from(&self.git_dir),
        }
    }
}

struct AnnotationRefreshJob {
    request_id: u32,
    project_id: String,
    chapter_id: String,
    text_revision: u64,
    text: String,
    context: AnnotationContext,
    callback: AnnotationWorkerCallback,
}

struct AnnotationApplyJob {
    request_id: u32,
    project_id: String,
    chapter_id: String,
    base_revision: u64,
    text_revision: u64,
    cause: String,
    ranges_json: String,
    context: AnnotationContext,
    callback: AnnotationWorkerCallback,
}

enum AnnotationJob {
    Refresh(AnnotationRefreshJob),
    Apply(AnnotationApplyJob),
}

enum AnnotationWorkerEvent {
    Snapshot {
        request_id: u32,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        result: Result<String, String>,
    },
    Edits {
        request_id: u32,
        project_id: String,
        chapter_id: String,
        base_revision: u64,
        result: Result<u64, String>,
    },
}

type AnnotationWorkerCallback = Box<dyn FnOnce(AnnotationWorkerEvent) + Send>;

#[derive(Clone)]
struct AnnotationSession {
    baseline_id: String,
    revision: u64,
}

#[derive(Default)]
struct AnnotationApiState {
    api: Option<WriterCoreApi>,
    sessions: HashMap<String, AnnotationSession>,
    pending: HashMap<String, VecDeque<AnnotationApplyJob>>,
}

#[derive(Default)]
struct AnnotationWorkerState {
    apis: HashMap<String, AnnotationApiState>,
}

impl AnnotationWorkerState {
    fn api_state(&mut self, context: &AnnotationContext) -> &mut AnnotationApiState {
        let key = context.key();
        self.apis.entry(key).or_insert_with(|| AnnotationApiState {
            api: Some(crate::backend::app_backend::with_layout_core_api(
                &context.data_root,
                &context.projects_root,
                &context.layout(),
            )),
            ..Default::default()
        })
    }
}

static ANNOTATION_WORKER: OnceLock<mpsc::Sender<AnnotationJob>> = OnceLock::new();

fn annotation_worker() -> &'static mpsc::Sender<AnnotationJob> {
    ANNOTATION_WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("annotation-mapping".to_string())
            .spawn(move || annotation_worker_loop(rx))
            .expect("annotation mapping worker must start");
        tx
    })
}

fn annotation_worker_loop(receiver: mpsc::Receiver<AnnotationJob>) {
    let mut state = AnnotationWorkerState::default();
    while let Ok(job) = receiver.recv() {
        match job {
            AnnotationJob::Refresh(job) => handle_annotation_refresh(&mut state, job),
            AnnotationJob::Apply(job) => handle_annotation_apply(&mut state, job),
        }
    }
}

fn chapter_key(project_id: &str, chapter_id: &str) -> String {
    format!("{project_id}\u{1f}{chapter_id}")
}

fn handle_annotation_refresh(state: &mut AnnotationWorkerState, job: AnnotationRefreshJob) {
    let api_state = state.api_state(&job.context);
    let key = chapter_key(&job.project_id, &job.chapter_id);
    let existing = api_state.sessions.get(&key).cloned();
    let preferred_baseline = existing
        .as_ref()
        .filter(|session| session.revision == job.text_revision)
        .map(|session| session.baseline_id.clone())
        .unwrap_or_default();
    let api = match api_state.api.as_ref() {
        Some(api) => api,
        None => {
            (job.callback)(AnnotationWorkerEvent::Snapshot {
                request_id: job.request_id,
                project_id: job.project_id,
                chapter_id: job.chapter_id,
                text_revision: job.text_revision,
                result: Err("标注后台 API 不可用".to_string()),
            });
            return;
        }
    };
    let first = api.get_chapter_annotations(
        job.project_id.clone(),
        job.chapter_id.clone(),
        job.text_revision,
        job.text.clone(),
        preferred_baseline.clone(),
    );
    let result = match first {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            let first_error = error.to_string();
            if preferred_baseline.is_empty() {
                Err(first_error)
            } else {
                api.get_chapter_annotations(
                    job.project_id.clone(),
                    job.chapter_id.clone(),
                    job.text_revision,
                    job.text.clone(),
                    String::new(),
                )
                .map_err(|retry_error| format!("{first_error}; retry: {retry_error}"))
            }
        }
    };
    match result {
        Ok(snapshot) => {
            let snapshot_json = match serde_json::to_string(&snapshot) {
                Ok(json) => json,
                Err(error) => {
                    (job.callback)(AnnotationWorkerEvent::Snapshot {
                        request_id: job.request_id,
                        project_id: job.project_id,
                        chapter_id: job.chapter_id,
                        text_revision: job.text_revision,
                        result: Err(format!("序列化标注快照失败: {error}")),
                    });
                    return;
                }
            };
            let should_update = existing
                .as_ref()
                .map(|session| session.revision <= snapshot.text_revision)
                .unwrap_or(true);
            if should_update {
                api_state.sessions.insert(
                    key.clone(),
                    AnnotationSession {
                        baseline_id: snapshot.baseline_id.clone(),
                        revision: snapshot.text_revision,
                    },
                );
            }
            (job.callback)(AnnotationWorkerEvent::Snapshot {
                request_id: job.request_id,
                project_id: job.project_id,
                chapter_id: job.chapter_id,
                text_revision: snapshot.text_revision,
                result: Ok(snapshot_json),
            });
            drain_pending_annotation_edits(api_state, &key);
        }
        Err(error) => {
            (job.callback)(AnnotationWorkerEvent::Snapshot {
                request_id: job.request_id,
                project_id: job.project_id,
                chapter_id: job.chapter_id,
                text_revision: job.text_revision,
                result: Err(error.to_string()),
            });
        }
    }
}

fn handle_annotation_apply(state: &mut AnnotationWorkerState, job: AnnotationApplyJob) {
    let api_state = state.api_state(&job.context);
    let key = chapter_key(&job.project_id, &job.chapter_id);
    if let Some(session) = api_state.sessions.get(&key) {
        if job.base_revision < session.revision {
            // 更晚的标注快照已经包含这笔编辑；只回报实际消费到的 revision。
            (job.callback)(AnnotationWorkerEvent::Edits {
                request_id: job.request_id,
                project_id: job.project_id,
                chapter_id: job.chapter_id,
                base_revision: job.base_revision,
                result: Ok(session.revision),
            });
            return;
        }
        if job.base_revision == session.revision {
            let revision = {
                let api = match api_state.api.as_ref() {
                    Some(api) => api,
                    None => {
                        (job.callback)(AnnotationWorkerEvent::Edits {
                            request_id: job.request_id,
                            project_id: job.project_id,
                            chapter_id: job.chapter_id,
                            base_revision: job.base_revision,
                            result: Err("标注后台 API 不可用".to_string()),
                        });
                        return;
                    }
                };
                let session = api_state
                    .sessions
                    .get_mut(&key)
                    .expect("annotation session checked above");
                apply_annotation_edit_job(api, session, &job)
            };
            match revision {
                Ok(revision) => {
                    (job.callback)(AnnotationWorkerEvent::Edits {
                        request_id: job.request_id,
                        project_id: job.project_id,
                        chapter_id: job.chapter_id,
                        base_revision: job.base_revision,
                        result: Ok(revision),
                    });
                    drain_pending_annotation_edits(api_state, &key);
                }
                Err(error) => {
                    (job.callback)(AnnotationWorkerEvent::Edits {
                        request_id: job.request_id,
                        project_id: job.project_id,
                        chapter_id: job.chapter_id,
                        base_revision: job.base_revision,
                        result: Err(error),
                    });
                }
            }
            return;
        }
    }
    // 基准还没建立，或前序编辑尚未完成：事实进入所属章节队列，等 refresh
    // 建立 baseline 后按 base/new revision 连续消费，不丢任务。
    api_state.pending.entry(key).or_default().push_back(job);
}

fn apply_annotation_edit_job(
    api: &WriterCoreApi,
    session: &mut AnnotationSession,
    job: &AnnotationApplyJob,
) -> Result<u64, String> {
    let cause = parse_annotation_cause(&job.cause)?;
    let edits = parse_annotation_ranges(&job.ranges_json)?;
    api.apply_annotation_edits(
        job.project_id.clone(),
        job.chapter_id.clone(),
        session.baseline_id.clone(),
        job.base_revision,
        job.text_revision,
        cause,
        edits,
    )
    .map_err(|error| error.to_string())?;
    session.revision = job.text_revision;
    Ok(session.revision)
}

fn drain_pending_annotation_edits(api_state: &mut AnnotationApiState, key: &str) {
    loop {
        let session_revision = match api_state.sessions.get(key) {
            Some(session) => session.revision,
            None => return,
        };
        let base_revision = match api_state
            .pending
            .get(key)
            .and_then(|queue| queue.front())
            .map(|job| job.base_revision)
        {
            Some(base_revision) => base_revision,
            None => return,
        };
        if base_revision > session_revision {
            return;
        }
        let job = api_state
            .pending
            .get_mut(key)
            .and_then(|queue| queue.pop_front())
            .expect("pending annotation job checked above");
        if base_revision < session_revision {
            (job.callback)(AnnotationWorkerEvent::Edits {
                request_id: job.request_id,
                project_id: job.project_id,
                chapter_id: job.chapter_id,
                base_revision: job.base_revision,
                result: Ok(session_revision),
            });
            continue;
        }
        let api = match api_state.api.as_ref() {
            Some(api) => api,
            None => {
                (job.callback)(AnnotationWorkerEvent::Edits {
                    request_id: job.request_id,
                    project_id: job.project_id,
                    chapter_id: job.chapter_id,
                    base_revision: job.base_revision,
                    result: Err("标注后台 API 不可用".to_string()),
                });
                continue;
            }
        };
        let session = api_state
            .sessions
            .get_mut(key)
            .expect("annotation session checked above");
        match apply_annotation_edit_job(api, session, &job) {
            Ok(revision) => (job.callback)(AnnotationWorkerEvent::Edits {
                request_id: job.request_id,
                project_id: job.project_id,
                chapter_id: job.chapter_id,
                base_revision: job.base_revision,
                result: Ok(revision),
            }),
            Err(error) => (job.callback)(AnnotationWorkerEvent::Edits {
                request_id: job.request_id,
                project_id: job.project_id,
                chapter_id: job.chapter_id,
                base_revision: job.base_revision,
                result: Err(error),
            }),
        }
    }
}

fn parse_annotation_cause(value: &str) -> Result<EditorTransactionCauseDto, String> {
    match value {
        "Typing" => Ok(EditorTransactionCauseDto::Typing),
        "Delete" => Ok(EditorTransactionCauseDto::Delete),
        "ImeComposition" => Ok(EditorTransactionCauseDto::ImeComposition),
        "TypingCommit" => Ok(EditorTransactionCauseDto::TypingCommit),
        "Paste" => Ok(EditorTransactionCauseDto::Paste),
        "Undo" => Ok(EditorTransactionCauseDto::Undo),
        "Redo" => Ok(EditorTransactionCauseDto::Redo),
        "Load" => Ok(EditorTransactionCauseDto::Load),
        "Format" => Ok(EditorTransactionCauseDto::Format),
        "Programmatic" => Ok(EditorTransactionCauseDto::Programmatic),
        other => Err(format!("未知编辑事务来源: {other}")),
    }
}

fn parse_annotation_ranges(value: &str) -> Result<Vec<AnnotationEditDto>, String> {
    let ranges: Vec<[u32; 4]> =
        serde_json::from_str(value).map_err(|error| format!("正文标记更新范围无效: {error}"))?;
    Ok(ranges
        .into_iter()
        .map(|range| AnnotationEditDto {
            old_start_byte: range[0],
            old_end_byte: range[1],
            new_start_byte: range[2],
            new_end_byte: range[3],
        })
        .collect())
}

use super::{AppBackend, AppRef};

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct AnnotationBackend {
    base: qt_base_class!(trait QObject),
    list_tags_json: qt_method!(fn(&self, project_id: QString) -> QString),
    list_named_entries_json: qt_method!(fn(&self, project_id: QString) -> QString),
    create_tag_json:
        qt_method!(fn(&self, project_id: QString, name: QString, color_json: QString) -> QString),
    rename_tag_json:
        qt_method!(fn(&self, project_id: QString, tag_id: QString, name: QString) -> QString),
    delete_tag_json: qt_method!(fn(&self, project_id: QString, tag_id: QString) -> QString),
    merge_tags_json: qt_method!(
        fn(&self, project_id: QString, source_tag_id: QString, target_tag_id: QString) -> QString
    ),
    set_tag_bindings_json: qt_method!(
        fn(&self, project_id: QString, target_json: QString, tag_ids_json: QString) -> QString
    ),
    list_bindings_for_target_json:
        qt_method!(fn(&self, project_id: QString, target_json: QString) -> QString),
    create_text_anchor_json: qt_method!(
        fn(
            &self,
            project_id: QString,
            chapter_id: QString,
            start_byte: u32,
            end_byte: u32,
            text_revision: u64,
            text: QString,
        ) -> QString
    ),
    create_named_entry_json: qt_method!(
        fn(
            &self,
            project_id: QString,
            canonical_name: QString,
            aliases_json: QString,
            misspellings_json: QString,
            auto_link_enabled: bool,
        ) -> QString
    ),
    update_named_entry_json: qt_method!(
        fn(
            &self,
            project_id: QString,
            entry_id: QString,
            canonical_name: QString,
            aliases_json: QString,
            misspellings_json: QString,
            auto_link_enabled: bool,
        ) -> QString
    ),
    delete_named_entry_json:
        qt_method!(fn(&self, project_id: QString, entry_id: QString) -> QString),
    create_link_json: qt_method!(fn(&self, source_json: QString, target_json: QString) -> QString),
    update_link_json: qt_method!(
        fn(
            &self,
            project_id: QString,
            link_id: QString,
            source_json: QString,
            target_json: QString,
        ) -> QString
    ),
    delete_link_json: qt_method!(fn(&self, project_id: QString, link_id: QString) -> QString),
    resolve_link_json: qt_method!(fn(&self, target_json: QString) -> QString),
    find_backlinks_json: qt_method!(fn(&self, target_json: QString) -> QString),
    list_links_for_source_json: qt_method!(fn(&self, source_json: QString) -> QString),
    get_text_anchor_json: qt_method!(fn(&self, project_id: QString, anchor_id: QString) -> QString),
    get_chapter_annotations_json: qt_method!(
        fn(
            &self,
            project_id: QString,
            chapter_id: QString,
            text_revision: u64,
            text: QString,
            baseline_id: QString,
        ) -> QString
    ),
    apply_annotation_edits_json: qt_method!(
        fn(
            &self,
            project_id: QString,
            chapter_id: QString,
            baseline_id: QString,
            base_revision: u64,
            text_revision: u64,
            cause: QString,
            edits_json: QString,
        ) -> QString
    ),
    request_chapter_annotations: qt_method!(
        fn(
            &self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            text_revision: u64,
            text: QString,
        )
    ),
    queue_annotation_edits: qt_method!(
        fn(
            &self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            base_revision: u64,
            text_revision: u64,
            cause: QString,
            ranges_json: QString,
        )
    ),
    find_auto_name_links_json: qt_method!(
        fn(
            &self,
            project_id: QString,
            chapter_id: QString,
            text_revision: u64,
            text: QString,
        ) -> QString
    ),
    annotation_chapter_ready: qt_signal!(
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        text_revision: u64,
        snapshot_json: QString
    ),
    annotation_chapter_failed: qt_signal!(
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        text_revision: u64,
        message: QString
    ),
    annotation_edits_applied: qt_signal!(
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        base_revision: u64,
        text_revision: u64,
        success: bool,
        message: QString
    ),
    app: AppRef,
}

impl AnnotationBackend {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }

    fn with_app<R>(
        &self,
        f: impl FnOnce(&AppBackend) -> R,
    ) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app(f)
    }

    fn execute<T: Serialize>(
        &self,
        operation: impl FnOnce(&WriterCoreApi) -> Result<T, WriterError>,
    ) -> QString {
        let result = match self.app.with_app(|app| app.core_api()) {
            Ok(Some(api)) => operation(&api),
            Ok(None) => Err(WriterError::Other("请先打开工作区".to_string())),
            Err(_) => Err(WriterError::Other("工作区状态暂不可用".to_string())),
        };
        ResultEnvelope::from_api_result(result)
            .to_json_string()
            .into()
    }

    fn decode<T: DeserializeOwned>(raw: &QString, label: &str) -> Result<T, WriterError> {
        serde_json::from_str(&raw.to_string())
            .map_err(|error| WriterError::Other(format!("{label} JSON 无效: {error}")))
    }

    fn decode_string_list(raw: &QString, label: &str) -> Result<Vec<String>, WriterError> {
        Self::decode(raw, label)
    }

    fn list_tags_json(&self, project_id: QString) -> QString {
        let project_id = project_id.to_string();
        self.execute(|api| api.list_tags(project_id.clone()))
    }

    fn list_named_entries_json(&self, project_id: QString) -> QString {
        let project_id = project_id.to_string();
        self.execute(|api| api.list_named_entries(project_id.clone()))
    }

    fn create_tag_json(&self, project_id: QString, name: QString, color_json: QString) -> QString {
        let project_id = project_id.to_string();
        let name = name.to_string();
        let color = Self::decode::<Option<String>>(&color_json, "color");
        match color {
            Ok(color) => {
                self.execute(|api| api.create_tag(project_id.clone(), name.clone(), color))
            }
            Err(error) => ResultEnvelope::<serde_json::Value>::error(error)
                .to_json_string()
                .into(),
        }
    }

    fn rename_tag_json(&self, project_id: QString, tag_id: QString, name: QString) -> QString {
        let (project_id, tag_id, name) =
            (project_id.to_string(), tag_id.to_string(), name.to_string());
        self.execute(|api| api.rename_tag(project_id.clone(), tag_id.clone(), name.clone()))
    }

    fn delete_tag_json(&self, project_id: QString, tag_id: QString) -> QString {
        let (project_id, tag_id) = (project_id.to_string(), tag_id.to_string());
        self.execute(|api| api.delete_tag(project_id.clone(), tag_id.clone()))
    }

    fn merge_tags_json(
        &self,
        project_id: QString,
        source_tag_id: QString,
        target_tag_id: QString,
    ) -> QString {
        let (project_id, source_tag_id, target_tag_id) = (
            project_id.to_string(),
            source_tag_id.to_string(),
            target_tag_id.to_string(),
        );
        self.execute(|api| {
            api.merge_tags(
                project_id.clone(),
                source_tag_id.clone(),
                target_tag_id.clone(),
            )
        })
    }

    fn set_tag_bindings_json(
        &self,
        project_id: QString,
        target_json: QString,
        tag_ids_json: QString,
    ) -> QString {
        let (project_id, target, tag_ids) = match (
            Self::decode::<AnnotationObjectRefDto>(&target_json, "target"),
            Self::decode_string_list(&tag_ids_json, "tagIds"),
        ) {
            (Ok(target), Ok(tag_ids)) => (project_id.to_string(), target, tag_ids),
            (Err(error), _) | (_, Err(error)) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        self.execute(|api| api.set_tag_bindings(project_id.clone(), target, tag_ids))
    }

    fn list_bindings_for_target_json(&self, project_id: QString, target_json: QString) -> QString {
        let target = match Self::decode::<AnnotationObjectRefDto>(&target_json, "target") {
            Ok(target) => target,
            Err(error) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        let project_id = project_id.to_string();
        self.execute(|api| api.list_bindings_for_target(project_id.clone(), target))
    }

    fn create_text_anchor_json(
        &self,
        project_id: QString,
        chapter_id: QString,
        start_byte: u32,
        end_byte: u32,
        text_revision: u64,
        text: QString,
    ) -> QString {
        let (project_id, chapter_id, text) = (
            project_id.to_string(),
            chapter_id.to_string(),
            text.to_string(),
        );
        self.execute(|api| {
            api.create_text_anchor(
                project_id.clone(),
                chapter_id.clone(),
                start_byte,
                end_byte,
                text_revision,
                text.clone(),
            )
        })
    }

    fn create_named_entry_json(
        &self,
        project_id: QString,
        canonical_name: QString,
        aliases_json: QString,
        misspellings_json: QString,
        auto_link_enabled: bool,
    ) -> QString {
        let (aliases, misspellings) = match (
            Self::decode_string_list(&aliases_json, "aliases"),
            Self::decode_string_list(&misspellings_json, "misspellings"),
        ) {
            (Ok(aliases), Ok(misspellings)) => (aliases, misspellings),
            (Err(error), _) | (_, Err(error)) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        let (project_id, canonical_name) = (project_id.to_string(), canonical_name.to_string());
        self.execute(|api| {
            api.create_named_entry(
                project_id.clone(),
                canonical_name.clone(),
                aliases,
                misspellings,
                auto_link_enabled,
            )
        })
    }

    fn update_named_entry_json(
        &self,
        project_id: QString,
        entry_id: QString,
        canonical_name: QString,
        aliases_json: QString,
        misspellings_json: QString,
        auto_link_enabled: bool,
    ) -> QString {
        let (aliases, misspellings) = match (
            Self::decode_string_list(&aliases_json, "aliases"),
            Self::decode_string_list(&misspellings_json, "misspellings"),
        ) {
            (Ok(aliases), Ok(misspellings)) => (aliases, misspellings),
            (Err(error), _) | (_, Err(error)) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        let (project_id, entry_id, canonical_name) = (
            project_id.to_string(),
            entry_id.to_string(),
            canonical_name.to_string(),
        );
        self.execute(|api| {
            api.update_named_entry(
                project_id.clone(),
                entry_id.clone(),
                canonical_name.clone(),
                aliases,
                misspellings,
                auto_link_enabled,
            )
        })
    }

    fn delete_named_entry_json(&self, project_id: QString, entry_id: QString) -> QString {
        let (project_id, entry_id) = (project_id.to_string(), entry_id.to_string());
        self.execute(|api| api.delete_named_entry(project_id.clone(), entry_id.clone()))
    }

    fn create_link_json(&self, source_json: QString, target_json: QString) -> QString {
        let (source, target) = match (
            Self::decode::<AnnotationObjectRefDto>(&source_json, "source"),
            Self::decode::<AnnotationTargetDto>(&target_json, "target"),
        ) {
            (Ok(source), Ok(target)) => (source, target),
            (Err(error), _) | (_, Err(error)) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        self.execute(|api| api.create_link(source, target))
    }

    fn update_link_json(
        &self,
        project_id: QString,
        link_id: QString,
        source_json: QString,
        target_json: QString,
    ) -> QString {
        let (source, target) = match (
            Self::decode::<AnnotationObjectRefDto>(&source_json, "source"),
            Self::decode::<AnnotationTargetDto>(&target_json, "target"),
        ) {
            (Ok(source), Ok(target)) => (source, target),
            (Err(error), _) | (_, Err(error)) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        let (project_id, link_id) = (project_id.to_string(), link_id.to_string());
        self.execute(|api| api.update_link(project_id.clone(), link_id.clone(), source, target))
    }

    fn delete_link_json(&self, project_id: QString, link_id: QString) -> QString {
        let (project_id, link_id) = (project_id.to_string(), link_id.to_string());
        self.execute(|api| api.delete_link(project_id.clone(), link_id.clone()))
    }

    fn resolve_link_json(&self, target_json: QString) -> QString {
        let target = match Self::decode::<AnnotationTargetDto>(&target_json, "target") {
            Ok(target) => target,
            Err(error) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        self.execute(|api| api.resolve_link(target))
    }

    fn find_backlinks_json(&self, target_json: QString) -> QString {
        let target = match Self::decode::<AnnotationTargetDto>(&target_json, "target") {
            Ok(target) => target,
            Err(error) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        self.execute(|api| api.find_backlinks(target))
    }

    fn list_links_for_source_json(&self, source_json: QString) -> QString {
        let source = match Self::decode::<AnnotationObjectRefDto>(&source_json, "source") {
            Ok(source) => source,
            Err(error) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        self.execute(|api| api.list_links_for_source(source))
    }

    fn get_text_anchor_json(&self, project_id: QString, anchor_id: QString) -> QString {
        let (project_id, anchor_id) = (project_id.to_string(), anchor_id.to_string());
        self.execute(|api| api.get_text_anchor(project_id.clone(), anchor_id.clone()))
    }

    fn get_chapter_annotations_json(
        &self,
        project_id: QString,
        chapter_id: QString,
        text_revision: u64,
        text: QString,
        baseline_id: QString,
    ) -> QString {
        let (project_id, chapter_id, text, baseline_id) = (
            project_id.to_string(),
            chapter_id.to_string(),
            text.to_string(),
            baseline_id.to_string(),
        );
        self.execute(|api| {
            api.get_chapter_annotations(
                project_id.clone(),
                chapter_id.clone(),
                text_revision,
                text.clone(),
                baseline_id.clone(),
            )
        })
    }

    fn apply_annotation_edits_json(
        &self,
        project_id: QString,
        chapter_id: QString,
        baseline_id: QString,
        base_revision: u64,
        text_revision: u64,
        cause: QString,
        edits_json: QString,
    ) -> QString {
        let edits = match Self::decode::<Vec<AnnotationEditDto>>(&edits_json, "edits") {
            Ok(edits) => edits,
            Err(error) => {
                return ResultEnvelope::<serde_json::Value>::error(error)
                    .to_json_string()
                    .into()
            }
        };
        let cause = match cause.to_string().as_str() {
            "Typing" => EditorTransactionCauseDto::Typing,
            "Delete" => EditorTransactionCauseDto::Delete,
            "ImeComposition" => EditorTransactionCauseDto::ImeComposition,
            "TypingCommit" => EditorTransactionCauseDto::TypingCommit,
            "Paste" => EditorTransactionCauseDto::Paste,
            "Undo" => EditorTransactionCauseDto::Undo,
            "Redo" => EditorTransactionCauseDto::Redo,
            "Load" => EditorTransactionCauseDto::Load,
            "Format" => EditorTransactionCauseDto::Format,
            "Programmatic" => EditorTransactionCauseDto::Programmatic,
            value => {
                return ResultEnvelope::<serde_json::Value>::error(WriterError::Other(format!(
                    "未知编辑事务来源: {value}"
                )))
                .to_json_string()
                .into()
            }
        };
        let (project_id, chapter_id, baseline_id) = (
            project_id.to_string(),
            chapter_id.to_string(),
            baseline_id.to_string(),
        );
        self.execute(|api| {
            api.apply_annotation_edits(
                project_id.clone(),
                chapter_id.clone(),
                baseline_id.clone(),
                base_revision,
                text_revision,
                cause,
                edits,
            )
        })
    }

    fn request_chapter_annotations(
        &self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        text_revision: u64,
        text: QString,
    ) {
        let context = match self.with_app(|app| app.annotation_task_context()) {
            Ok(Some(context)) => context,
            Ok(None) => {
                self.annotation_chapter_failed(
                    request_id,
                    project_id,
                    chapter_id,
                    text_revision,
                    QString::from("工作区尚未打开"),
                );
                return;
            }
            Err(error) => {
                self.annotation_chapter_failed(
                    request_id,
                    project_id,
                    chapter_id,
                    text_revision,
                    QString::from(error.to_string()),
                );
                return;
            }
        };
        let (data_root, projects_root, layout) = context;
        let annotation_context = AnnotationContext::new(&data_root, &projects_root, &layout);
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |event: AnnotationWorkerEvent| {
            if let Some(pinned) = qptr.as_pinned() {
                let this = pinned.borrow_mut();
                match event {
                    AnnotationWorkerEvent::Snapshot {
                        request_id,
                        project_id,
                        chapter_id,
                        text_revision,
                        result,
                    } => match result {
                        Ok(snapshot_json) => this.annotation_chapter_ready(
                            request_id,
                            QString::from(project_id),
                            QString::from(chapter_id),
                            text_revision,
                            QString::from(snapshot_json),
                        ),
                        Err(message) => this.annotation_chapter_failed(
                            request_id,
                            QString::from(project_id),
                            QString::from(chapter_id),
                            text_revision,
                            QString::from(message),
                        ),
                    },
                    AnnotationWorkerEvent::Edits { .. } => {}
                }
            }
        });
        let fail_project = project_id.clone();
        let fail_chapter = chapter_id.clone();
        let job = AnnotationRefreshJob {
            request_id,
            project_id: project_id.to_string(),
            chapter_id: chapter_id.to_string(),
            text_revision,
            text: text.to_string(),
            context: annotation_context,
            callback: Box::new(move |event| callback(event)),
        };
        if let Err(error) = annotation_worker().send(AnnotationJob::Refresh(job)) {
            self.annotation_chapter_failed(
                request_id,
                fail_project,
                fail_chapter,
                text_revision,
                QString::from(error.to_string()),
            );
        }
    }

    fn queue_annotation_edits(
        &self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        base_revision: u64,
        text_revision: u64,
        cause: QString,
        ranges_json: QString,
    ) {
        let context = match self.with_app(|app| app.annotation_task_context()) {
            Ok(Some(context)) => context,
            Ok(None) => {
                self.annotation_edits_applied(
                    request_id,
                    project_id,
                    chapter_id,
                    base_revision,
                    text_revision,
                    false,
                    QString::from("工作区尚未打开"),
                );
                return;
            }
            Err(error) => {
                self.annotation_edits_applied(
                    request_id,
                    project_id,
                    chapter_id,
                    base_revision,
                    text_revision,
                    false,
                    QString::from(error.to_string()),
                );
                return;
            }
        };
        let (data_root, projects_root, layout) = context;
        let annotation_context = AnnotationContext::new(&data_root, &projects_root, &layout);
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |event: AnnotationWorkerEvent| {
            if let Some(pinned) = qptr.as_pinned() {
                let this = pinned.borrow_mut();
                if let AnnotationWorkerEvent::Edits {
                    request_id,
                    project_id,
                    chapter_id,
                    base_revision,
                    result,
                } = event
                {
                    match result {
                        Ok(text_revision) => this.annotation_edits_applied(
                            request_id,
                            QString::from(project_id),
                            QString::from(chapter_id),
                            base_revision,
                            text_revision,
                            true,
                            QString::from(""),
                        ),
                        Err(message) => this.annotation_edits_applied(
                            request_id,
                            QString::from(project_id),
                            QString::from(chapter_id),
                            base_revision,
                            base_revision,
                            false,
                            QString::from(message),
                        ),
                    }
                }
            }
        });
        let fail_project = project_id.clone();
        let fail_chapter = chapter_id.clone();
        let job = AnnotationApplyJob {
            request_id,
            project_id: project_id.to_string(),
            chapter_id: chapter_id.to_string(),
            base_revision,
            text_revision,
            cause: cause.to_string(),
            ranges_json: ranges_json.to_string(),
            context: annotation_context,
            callback: Box::new(move |event| callback(event)),
        };
        if let Err(error) = annotation_worker().send(AnnotationJob::Apply(job)) {
            self.annotation_edits_applied(
                request_id,
                fail_project,
                fail_chapter,
                base_revision,
                base_revision,
                false,
                QString::from(error.to_string()),
            );
        }
    }

    fn find_auto_name_links_json(
        &self,
        project_id: QString,
        chapter_id: QString,
        text_revision: u64,
        text: QString,
    ) -> QString {
        let (project_id, chapter_id, text) = (
            project_id.to_string(),
            chapter_id.to_string(),
            text.to_string(),
        );
        self.execute(|api| {
            api.find_auto_name_links(
                project_id.clone(),
                chapter_id.clone(),
                text_revision,
                text.clone(),
            )
        })
    }
}
