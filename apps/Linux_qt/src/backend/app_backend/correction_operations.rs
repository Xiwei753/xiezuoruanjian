//! Linux Qt proofreading adapter.
//!
//! The editor remains owned by its existing `SujianEditorItem` Core kernel.
//! This module only sends immutable text snapshots to WriterCoreApi. QML maps
//! the returned UTF-8 byte-range endpoints onto Qt's UTF-16 string positions.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use super::*;
use writer_core::api::types::{
    CorrectionDictionaryEntryKindDto, CorrectionEditRangeDto, CorrectionIgnoreScopeDto,
};

/// Issue #958：RPM 包内自带的中文 KenLM 模型安装路径。
///
/// RPM spec 把 `people2014corpus_chars.klm` 装到 `%{_datadir}/sujian/models/`，
/// 即 `/usr/share/sujian/models/`。运行时直接从这个只读安装目录 mmap 读取，
/// 不做每用户拷贝。源码运行/开发环境通常没有这个文件，见
/// `provision_bundled_correction_model`。
pub(crate) const BUNDLED_CORRECTION_MODEL_PATH: &str =
    "/usr/share/sujian/models/people2014corpus_chars.klm";

const CORRECTION_DICTIONARY_FILES: [&str; 8] = [
    "word_freq.txt",
    "same_pinyin.txt",
    "same_stroke.txt",
    "custom_confusion.txt",
    "person_name.txt",
    "place_name.txt",
    "proper_name.txt",
    "ignored_words.txt",
];

pub(crate) type CorrectionQueryCallback =
    Box<dyn FnOnce(u32, u64, Result<String, String>) + Send + Sync>;

/// A bounded, process-wide correction executor. One FIFO thread serializes
/// model work for every chapter; lightweight edit mappings run on the caller's
/// thread and never wait for that thread. The executor owns no per-chapter
/// sender or idle thread, so browsing chapters cannot accumulate threads.
struct CorrectionJob {
    project_id: String,
    chapter_id: String,
    session_id: u64,
    text_revision: u64,
    coalescable: bool,
    run: Box<dyn FnOnce() + Send>,
}

struct CorrectionQueue {
    jobs: Mutex<VecDeque<CorrectionJob>>,
    wake: Condvar,
    /// Newest `(session, revision)` submitted for each chapter. Used only to
    /// skip obsolete display/scan jobs; edit mappings are never skipped.
    latest: Mutex<HashMap<(String, String), (u64, u64)>>,
}

static CORRECTION_QUEUE: OnceLock<Arc<CorrectionQueue>> = OnceLock::new();

fn correction_queue() -> &'static Arc<CorrectionQueue> {
    CORRECTION_QUEUE.get_or_init(|| {
        let queue = Arc::new(CorrectionQueue {
            jobs: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            latest: Mutex::new(HashMap::new()),
        });
        let worker = Arc::clone(&queue);
        if let Err(error) = std::thread::Builder::new()
            .name("correction-executor".to_string())
            .spawn(move || correction_executor(worker))
        {
            log::error!("failed to start correction executor: {error}");
        }
        queue
    })
}

fn correction_executor(queue: Arc<CorrectionQueue>) {
    loop {
        let mut jobs = match queue.jobs.lock() {
            Ok(jobs) => jobs,
            Err(poisoned) => poisoned.into_inner(),
        };
        while jobs.is_empty() {
            jobs = match queue.wake.wait(jobs) {
                Ok(jobs) => jobs,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
        let job = match jobs.pop_front() {
            Some(job) => job,
            None => continue,
        };
        drop(jobs);

        if job.coalescable {
            let latest = queue
                .latest
                .lock()
                .map(|latest| {
                    latest
                        .get(&(job.project_id.clone(), job.chapter_id.clone()))
                        .copied()
                })
                .unwrap_or(None);
            if latest.is_some_and(|(session_id, text_revision)| {
                job.session_id < session_id
                    || (job.session_id == session_id && job.text_revision < text_revision)
            }) {
                continue;
            }
        }
        (job.run)();
    }
}

fn record_correction_latest(
    project_id: &str,
    chapter_id: &str,
    session_id: u64,
    text_revision: u64,
) {
    let queue = correction_queue();
    let mut latest = match queue.latest.lock() {
        Ok(latest) => latest,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entry = latest
        .entry((project_id.to_string(), chapter_id.to_string()))
        .or_insert((session_id, text_revision));
    if session_id < entry.0 || (session_id == entry.0 && text_revision < entry.1) {
        return;
    }
    *entry = (session_id, text_revision);
}

fn submit_correction_job(
    project_id: String,
    chapter_id: String,
    session_id: u64,
    text_revision: u64,
    coalescable: bool,
    run: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    record_correction_latest(&project_id, &chapter_id, session_id, text_revision);
    let queue = correction_queue();
    let mut jobs = queue
        .jobs
        .lock()
        .map_err(|_| "校对任务队列锁已损坏".to_string())?;
    jobs.push_back(CorrectionJob {
        project_id,
        chapter_id,
        session_id,
        text_revision,
        coalescable,
        run: Box::new(run),
    });
    queue.wake.notify_one();
    Ok(())
}

/// Drops a closed chapter's queue bookkeeping. Queued work for the closing
/// chapter is discarded so opening many chapters never grows the `latest`
/// map or leaves obsolete jobs waiting behind the shared executor.
pub(crate) fn release_correction_queue(project_id: &str, chapter_id: &str) {
    let queue = correction_queue();
    let mut latest = match queue.latest.lock() {
        Ok(latest) => latest,
        Err(poisoned) => poisoned.into_inner(),
    };
    latest.remove(&(project_id.to_string(), chapter_id.to_string()));
    let mut jobs = match queue.jobs.lock() {
        Ok(jobs) => jobs,
        Err(poisoned) => poisoned.into_inner(),
    };
    jobs.retain(|job| job.project_id != project_id || job.chapter_id != chapter_id);
}

fn parse_correction_edits_json(edits_json: &str) -> Result<Vec<CorrectionEditRangeDto>, String> {
    let ranges = serde_json::from_str::<Vec<[u32; 4]>>(edits_json)
        .map_err(|error| format!("校对编辑范围格式无效: {error}"))?;
    Ok(ranges
        .into_iter()
        .map(
            |[old_start, old_end, new_start, new_end]| CorrectionEditRangeDto {
                old_start_byte: old_start,
                old_end_byte: old_end,
                new_start_byte: new_start,
                new_end_byte: new_end,
            },
        )
        .collect())
}

impl AppBackend {
    pub(crate) fn correction_dictionary_resource_status(
        &self,
    ) -> Result<Vec<(String, bool)>, String> {
        if !self.current_has_data_root || self.current_data_root.is_empty() {
            return Err("Core 尚未打开工作区".to_string());
        }
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        let status = api
            .get_correction_resource_status()
            .map_err(|error| error.to_string())?;
        let mut resources = CORRECTION_DICTIONARY_FILES
            .iter()
            .map(|name| {
                let ready = status
                    .installed_dictionary_files
                    .iter()
                    .any(|installed| installed == name);
                ((*name).to_string(), ready)
            })
            .collect::<Vec<_>>();
        resources.push(("内置基础词库".to_string(), status.bundled_lexicon_available));
        Ok(resources)
    }

    pub(crate) fn import_correction_dictionary(
        &self,
        source_path: std::path::PathBuf,
    ) -> Result<String, String> {
        if !self.current_has_data_root || self.current_data_root.is_empty() {
            return Err("Core 尚未打开工作区".to_string());
        }

        let source = source_path.as_path();
        let file_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "所选词库文件名无效".to_string())?;
        if !CORRECTION_DICTIONARY_FILES.contains(&file_name) {
            return Err(format!("不支持导入词库文件：{file_name}"));
        }

        let source = source
            .canonicalize()
            .map_err(|error| format!("无法读取所选词库文件: {error}"))?;
        if !source.is_file() {
            return Err("所选词库路径不是文件".to_string());
        }

        // This is the same data-root string passed to WriterCoreApi; Core reads
        // its correction resources from app_data_root/correction-data.
        let resource_root = std::path::Path::new(&self.current_data_root).join("correction-data");
        std::fs::create_dir_all(&resource_root)
            .map_err(|error| format!("无法创建校对词库目录: {error}"))?;
        let destination = resource_root.join(file_name);

        if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
            if metadata.file_type().is_symlink() {
                return Err("目标词库文件是符号链接，已取消覆盖".to_string());
            }
            if !metadata.is_file() {
                return Err("目标词库路径不是普通文件".to_string());
            }
            if destination.canonicalize().ok().as_deref() == Some(source.as_path()) {
                return Ok(file_name.to_string());
            }
        }

        std::fs::copy(&source, &destination)
            .map_err(|error| format!("复制词库文件失败: {error}"))?;
        Ok(file_name.to_string())
    }

    pub(crate) fn set_correction_model_path(&self, path: String) -> Result<(), String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("模型路径不能为空".to_string());
        }
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        api.set_correction_model_path(path.to_string())
            .map_err(|error| error.to_string())
    }

    /// Issue #958：把 RPM 安装的内置中文 KenLM 模型作为兜底交给 Core。
    ///
    /// 判定语义：
    /// - 内置模型文件不存在（源码运行/开发环境）→ 什么都不做；
    /// - Core 已配置过模型（`model_configured == true`，即使配置文件当前缺失）→
    ///   直接返回，绝不覆盖用户自己的模型设置；
    /// - 仅当 Core 从未配置过模型、且内置文件存在时，才把内置安装路径交给 Core。
    ///
    /// 由打开/切换工作区的启动路径调用一次。首次写入后 Core 会把该路径持久化，
    /// 后续启动 `model_configured` 即为 true，不会反复覆盖。
    pub(crate) fn provision_bundled_correction_model(&self) -> Result<(), String> {
        if !std::path::Path::new(BUNDLED_CORRECTION_MODEL_PATH).is_file() {
            return Ok(());
        }
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        let status = api
            .get_correction_resource_status()
            .map_err(|error| error.to_string())?;
        if status.model_configured {
            return Ok(());
        }
        api.set_correction_model_path(BUNDLED_CORRECTION_MODEL_PATH.to_string())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn ignore_correction(
        &self,
        project_id: String,
        chapter_id: String,
        rule_id: String,
        original: String,
        scope: String,
        text_revision: u64,
        start_byte: u32,
        end_byte: u32,
    ) -> Result<bool, String> {
        let scope = match scope.as_str() {
            "once" => CorrectionIgnoreScopeDto::Once,
            "chapter" => CorrectionIgnoreScopeDto::Chapter,
            "rule" => CorrectionIgnoreScopeDto::Rule,
            _ => return Err("不支持的忽略范围".to_string()),
        };
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        api.ignore_correction(
            project_id,
            chapter_id,
            rule_id,
            original,
            scope,
            text_revision,
            start_byte,
            end_byte,
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn add_correction_dictionary_entry(
        &self,
        project_id: String,
        entry: String,
    ) -> Result<bool, String> {
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        api.add_correction_dictionary_entry(project_id, entry)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn add_correction_dictionary_entry_with_kind(
        &self,
        project_id: String,
        entry: String,
        kind: CorrectionDictionaryEntryKindDto,
    ) -> Result<bool, String> {
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        api.add_correction_dictionary_entry_with_kind(project_id, entry, kind)
            .map_err(|error| error.to_string())
    }

    /// Queue one whole-chapter proofreading request without blocking the Qt UI.
    ///
    /// Issue #954: Core returns a merged chapter snapshot. Segments whose
    /// content and context fingerprints stay valid reuse previous results; only
    /// dirty or never-checked segments reach the model. Results carry the
    /// submitted revision; `EditorController` drops them if the active chapter
    /// or the `SujianEditorItem` revision has since changed.
    pub(crate) fn request_correction_chapter(
        &self,
        request_id: u32,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        text: String,
        completion: CorrectionQueryCallback,
    ) -> Result<(), String> {
        let (data_root, projects_root, layout) =
            if self.current_has_data_root && !self.current_data_root.is_empty() {
                let layout = self
                    .current_workspace_git_layout
                    .clone()
                    .ok_or_else(|| "当前工作区缺少 Core 存储布局".to_string())?;
                (
                    self.current_data_root.clone(),
                    self.current_projects_root.clone(),
                    layout,
                )
            } else {
                return Err("Core 尚未打开工作区".to_string());
            };

        submit_correction_job(
            project_id.clone(),
            chapter_id.clone(),
            session_id,
            text_revision,
            false,
            move || {
                let api = with_layout_core_api(&data_root, &projects_root, &layout);
                // The Core API receives an owned snapshot so model loading and
                // chapter scanning never block typing, layout, or saving.
                let result = api
                    .check_correction_chapter(
                        project_id,
                        chapter_id,
                        session_id,
                        text_revision,
                        text,
                    )
                    .map_err(|error| error.to_string())
                    .and_then(|suggestions| {
                        serde_json::to_string(&suggestions)
                            .map_err(|error| format!("无法编码校对结果: {error}"))
                    });
                completion(request_id, text_revision, result);
            },
        )
    }

    /// Issue #954: read the still-valid cached proofreading results for the
    /// current chapter snapshot. This never runs the model: Core re-derives
    /// every wave position from the current text fingerprints, so insert and
    /// delete edits only shift byte offsets. `EditorController` uses it to
    /// restore waves instantly on chapter open; edits go through
    /// `request_correction_edits_apply` and `request_correction_pending` so
    /// Core maps only changed ranges and scans only dirty segments.
    pub(crate) fn request_correction_snapshot(
        &self,
        request_id: u32,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        text: String,
        completion: CorrectionQueryCallback,
    ) -> Result<(), String> {
        let (data_root, projects_root, layout) =
            if self.current_has_data_root && !self.current_data_root.is_empty() {
                let layout = self
                    .current_workspace_git_layout
                    .clone()
                    .ok_or_else(|| "当前工作区缺少 Core 存储布局".to_string())?;
                (
                    self.current_data_root.clone(),
                    self.current_projects_root.clone(),
                    layout,
                )
            } else {
                return Err("Core 尚未打开工作区".to_string());
            };

        submit_correction_job(
            project_id.clone(),
            chapter_id.clone(),
            session_id,
            text_revision,
            true,
            move || {
                let api = with_layout_core_api(&data_root, &projects_root, &layout);
                let result = api
                    .correction_chapter_snapshot(
                        project_id,
                        chapter_id,
                        session_id,
                        text_revision,
                        text,
                    )
                    .map_err(|error| error.to_string())
                    .and_then(|snapshot| {
                        serde_json::to_string(&snapshot)
                            .map_err(|error| format!("无法编码校对结果: {error}"))
                    });
                completion(request_id, text_revision, result);
            },
        )
    }

    /// Queue proofreading for one UTF-8 byte range in an immutable chapter
    /// snapshot. Core expands the range with nearby context and returns only
    /// suggestions whose source spans fall inside the requested range.
    ///
    /// Issue #954: the response is the merged chapter snapshot. Unchanged
    /// segments keep their cached suggestions, so QML replaces its list
    /// wholesale instead of shifting or merging locally.
    pub(crate) fn request_correction_segment(
        &self,
        request_id: u32,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        text: String,
        start_byte: u32,
        end_byte: u32,
        completion: CorrectionQueryCallback,
    ) -> Result<(), String> {
        let (data_root, projects_root, layout) =
            if self.current_has_data_root && !self.current_data_root.is_empty() {
                let layout = self
                    .current_workspace_git_layout
                    .clone()
                    .ok_or_else(|| "当前工作区缺少 Core 存储布局".to_string())?;
                (
                    self.current_data_root.clone(),
                    self.current_projects_root.clone(),
                    layout,
                )
            } else {
                return Err("Core 尚未打开工作区".to_string());
            };

        submit_correction_job(
            project_id.clone(),
            chapter_id.clone(),
            session_id,
            text_revision,
            true,
            move || {
                let api = with_layout_core_api(&data_root, &projects_root, &layout);
                let result = api
                    .check_correction_segment(
                        project_id,
                        chapter_id,
                        session_id,
                        text_revision,
                        text,
                        start_byte,
                        end_byte,
                    )
                    .map_err(|error| error.to_string())
                    .and_then(|suggestions| {
                        serde_json::to_string(&suggestions)
                            .map_err(|error| format!("无法编码校对结果: {error}"))
                    });
                completion(request_id, text_revision, result);
            },
        )
    }

    /// Lightweight half of the two-stage edit flow. Runs on the caller thread
    /// and only maps byte ranges in the committed segment index; it does not
    /// copy the chapter or touch the model.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request_correction_edits_apply(
        &self,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        base_revision: u64,
        edits_json: String,
    ) -> Result<(), String> {
        let layout = if self.current_has_data_root && !self.current_data_root.is_empty() {
            self.current_workspace_git_layout
                .clone()
                .ok_or_else(|| "当前工作区缺少 Core 存储布局".to_string())?
        } else {
            return Err("Core 尚未打开工作区".to_string());
        };
        let edits = parse_correction_edits_json(&edits_json)?;
        let api = with_layout_core_api(
            &self.current_data_root,
            &self.current_projects_root,
            &layout,
        );
        api.apply_correction_edits(
            project_id.clone(),
            chapter_id.clone(),
            session_id,
            text_revision,
            base_revision,
            edits,
        )
        .map_err(|error| error.to_string())?;
        record_correction_latest(&project_id, &chapter_id, session_id, text_revision);
        Ok(())
    }

    /// Model half of the two-stage edit flow. The executor coalesces queued
    /// scans and only the newest revision that still matches the editor
    /// snapshot reaches Core; an older queued scan is skipped.
    pub(crate) fn request_correction_pending(
        &self,
        request_id: u32,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        text: String,
        completion: CorrectionQueryCallback,
    ) -> Result<(), String> {
        let (data_root, projects_root, layout) =
            if self.current_has_data_root && !self.current_data_root.is_empty() {
                let layout = self
                    .current_workspace_git_layout
                    .clone()
                    .ok_or_else(|| "当前工作区缺少 Core 存储布局".to_string())?;
                (
                    self.current_data_root.clone(),
                    self.current_projects_root.clone(),
                    layout,
                )
            } else {
                return Err("Core 尚未打开工作区".to_string());
            };
        submit_correction_job(
            project_id.clone(),
            chapter_id.clone(),
            session_id,
            text_revision,
            true,
            move || {
                let api = with_layout_core_api(&data_root, &projects_root, &layout);
                let result = api
                    .scan_correction_pending(
                        project_id,
                        chapter_id,
                        session_id,
                        text_revision,
                        text,
                    )
                    .map_err(|error| error.to_string())
                    .and_then(|snapshot| {
                        serde_json::to_string(&snapshot)
                            .map_err(|error| format!("无法编码校对结果: {error}"))
                    });
                completion(request_id, text_revision, result);
            },
        )
    }

    /// Issue #954: apply the editor transaction's real patch ranges. Core shifts
    /// untouched cached segments in memory and rescans only changed sentences.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request_correction_edits(
        &self,
        request_id: u32,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        base_revision: u64,
        edits_json: String,
        text: String,
        completion: CorrectionQueryCallback,
    ) -> Result<(), String> {
        let (data_root, projects_root, layout) =
            if self.current_has_data_root && !self.current_data_root.is_empty() {
                let layout = self
                    .current_workspace_git_layout
                    .clone()
                    .ok_or_else(|| "当前工作区缺少 Core 存储布局".to_string())?;
                (
                    self.current_data_root.clone(),
                    self.current_projects_root.clone(),
                    layout,
                )
            } else {
                return Err("Core 尚未打开工作区".to_string());
            };

        // Edit transactions must reach Core in revision order. Skipping one
        // would leave a revision gap, forcing a whole-chapter reconcile and
        // losing the per-edit incremental mapping.
        submit_correction_job(
            project_id.clone(),
            chapter_id.clone(),
            session_id,
            text_revision,
            false,
            move || {
                let api = with_layout_core_api(&data_root, &projects_root, &layout);
                let result = serde_json::from_str::<Vec<[u32; 4]>>(&edits_json)
                    .map_err(|error| format!("校对编辑范围格式无效: {error}"))
                    .and_then(|ranges| {
                        let edits = ranges
                            .into_iter()
                            .map(|[old_start, old_end, new_start, new_end]| {
                                writer_core::api::types::CorrectionEditRangeDto {
                                    old_start_byte: old_start,
                                    old_end_byte: old_end,
                                    new_start_byte: new_start,
                                    new_end_byte: new_end,
                                }
                            })
                            .collect();
                        api.check_correction_edits(
                            project_id,
                            chapter_id,
                            session_id,
                            text_revision,
                            base_revision,
                            text,
                            edits,
                        )
                        .map_err(|error| error.to_string())
                    })
                    .and_then(|snapshot| {
                        serde_json::to_string(&snapshot)
                            .map_err(|error| format!("无法编码校对结果: {error}"))
                    });
                completion(request_id, text_revision, result);
            },
        )
    }
}
