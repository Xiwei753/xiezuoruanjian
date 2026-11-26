//! Bounded correction executor and queue coalescing.
//!
//! Kept separate from the Core-facing operations so the adapter file stays
//! within the production-file budget and queue ownership has one home.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

/// Which queue slot one correction job occupies.
///
/// Issue #1062 三.3：只保留当前编辑会话最新的待校对 revision 与脏区。扫描与
/// 恢复显示任务可以被新请求取代；编辑映射必须按 revision 顺序全部执行，否则
/// Core 的段索引会出现 revision 缺口。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CorrectionJobClass {
    /// 轻量编辑映射（`edits` / `edit_apply`）：不跑模型，只把事务的真实改动
    /// 区应用到 Core 的段索引，永远不能被丢弃。
    EditMapping,
    /// 模型扫描（`pending` / `chapter_all` / `segment`）：只保留该章当前编辑
    /// 会话最新的 revision，旧 revision 在窗口边界退出。
    Scan,
    /// restore-only 缓存读取（`snapshot`）：不跑模型，也不代表任何 revision
    /// 已经扫描完成，因此它可以被更新的扫描取代，但绝不能反过来取代扫描。
    Display,
}

/// A bounded, process-wide correction executor. One FIFO thread serializes
/// model work for every chapter; lightweight edit mappings run on the caller's
/// thread and never wait for that thread. The executor owns no per-chapter
/// sender or idle thread, so browsing chapters cannot accumulate threads.
struct CorrectionJob {
    project_id: String,
    chapter_id: String,
    request_id: u32,
    session_id: u64,
    text_revision: u64,
    kind: &'static str,
    queued_at: Instant,
    class: CorrectionJobClass,
    run: Box<dyn FnOnce() -> bool + Send>,
}

struct CorrectionQueue {
    jobs: Mutex<VecDeque<CorrectionJob>>,
    wake: Condvar,
    /// Newest `(session, revision)` a real scan was submitted for, per chapter.
    /// Used only to skip obsolete scan and display jobs; edit mappings are never
    /// skipped. restore-only displays deliberately never publish here: they do
    /// not scan, so they must not make a revision look checked.
    latest: Mutex<HashMap<(String, String), (u64, u64)>>,
    /// Document that most recently asked for proofreading. A coalescable job
    /// popped after the writer already left that chapter must not initialize a
    /// 474 MB model before returning results nobody can use.
    active_document: Mutex<Option<(String, String, u64)>>,
}

static CORRECTION_QUEUE: OnceLock<Arc<CorrectionQueue>> = OnceLock::new();

fn correction_queue() -> &'static Arc<CorrectionQueue> {
    CORRECTION_QUEUE.get_or_init(|| {
        let queue = Arc::new(CorrectionQueue {
            jobs: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            latest: Mutex::new(HashMap::new()),
            active_document: Mutex::new(None),
        });
        let worker = Arc::clone(&queue);
        if std::thread::Builder::new()
            .name("correction-executor".to_string())
            .spawn(move || correction_executor(worker))
            .is_err()
        {
            log::error!(target: "correction.job", "correction.job.worker_start_failed");
        }
        queue
    })
}

fn record_correction_job_event(
    event: &str,
    request_id: u32,
    session_id: u64,
    text_revision: u64,
    kind: &'static str,
    result: Option<&str>,
    reason: Option<&str>,
    queue_wait_ms: Option<u128>,
    execution_ms: Option<u128>,
    queue_depth: Option<usize>,
) {
    let mut fields = vec![
        ("request_id", request_id.to_string()),
        ("session_id", session_id.to_string()),
        ("text_revision", text_revision.to_string()),
        ("job_kind", kind.to_string()),
    ];
    if let Some(result) = result {
        fields.push(("result", result.to_string()));
    }
    if let Some(reason) = reason {
        fields.push(("reason", reason.to_string()));
    }
    if let Some(queue_wait_ms) = queue_wait_ms {
        fields.push(("queue_wait_ms", queue_wait_ms.to_string()));
    }
    if let Some(execution_ms) = execution_ms {
        fields.push(("execution_ms", execution_ms.to_string()));
    }
    if let Some(queue_depth) = queue_depth {
        fields.push(("queue_depth", queue_depth.to_string()));
    }
    let field_refs = fields
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect::<Vec<_>>();
    super::record_struct_event(
        writer_diagnostics::DiagnosticOrigin::App,
        event,
        "correction.job",
        &field_refs,
    );
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
        let queue_depth = jobs.len();
        drop(jobs);

        if job.class != CorrectionJobClass::EditMapping {
            let latest = queue
                .latest
                .lock()
                .map(|latest| {
                    latest
                        .get(&(job.project_id.clone(), job.chapter_id.clone()))
                        .copied()
                })
                .unwrap_or(None);
            // 窗口边界：每次扫描窗口开始前重新读一次 latest revision。扫描任务
            // 被同一章更新的 revision 取代；restore-only 只比最新的真实扫描旧时
            // 才跳过，它自己的提交不会推进 latest，因此不可能把扫描标记为已完成。
            let superseded = latest.is_some_and(|(session_id, text_revision)| {
                job.session_id < session_id
                    || (job.session_id == session_id && job.text_revision < text_revision)
            });
            // The job may have been popped while an older chapter was still
            // current, or the chapter may have been left while this job waited
            // in the queue. Check again immediately before the expensive Core
            // call so a stale chapter never opens the model.
            let stale_document = correction_job_document_obsolete(&job);
            if superseded || stale_document {
                record_correction_job_event(
                    "correction.job.skipped",
                    job.request_id,
                    job.session_id,
                    job.text_revision,
                    job.kind,
                    Some("skipped"),
                    Some(if stale_document {
                        "stale_document"
                    } else {
                        "superseded_revision"
                    }),
                    Some(job.queued_at.elapsed().as_millis()),
                    None,
                    Some(queue_depth),
                );
                continue;
            }
        }
        let queue_wait_ms = job.queued_at.elapsed().as_millis();
        record_correction_job_event(
            "correction.job.start",
            job.request_id,
            job.session_id,
            job.text_revision,
            job.kind,
            None,
            None,
            Some(queue_wait_ms),
            None,
            Some(queue_depth),
        );
        let execution_started_at = Instant::now();
        // 失败/panic 只能落在校对诊断里：执行器线程必须活到下一个任务，否则
        // 正文保存 actor 之外的校对通道会永久停摆。用 `Mutex` 作为显式的
        // panic 边界（毒化后仍可取回闭包），不靠 `AssertUnwindSafe` 掩盖
        // 任务闭包里的回调/借用状态。
        let run = std::sync::Mutex::new(job.run);
        let run_result = std::panic::catch_unwind(move || {
            let run = run
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            run()
        });
        let succeeded = matches!(run_result, Ok(true));
        let failure_reason = match &run_result {
            Ok(true) => None,
            Ok(false) => Some("core_or_encoding_error"),
            Err(_) => Some("job_panicked"),
        };
        record_correction_job_event(
            "correction.job.end",
            job.request_id,
            job.session_id,
            job.text_revision,
            job.kind,
            Some(if succeeded { "success" } else { "error" }),
            failure_reason,
            Some(queue_wait_ms),
            Some(execution_started_at.elapsed().as_millis()),
            Some(queue_depth),
        );
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

fn record_active_correction_document(project_id: &str, chapter_id: &str, session_id: u64) {
    let queue = correction_queue();
    if let Ok(mut active) = queue.active_document.lock() {
        *active = Some((project_id.to_string(), chapter_id.to_string(), session_id));
    }
}

/// True when this job no longer belongs to the document the writer is looking
/// at. Only coalescable display/scan jobs consult it; edit mappings must keep
/// their revision order even after the chapter changed.
///
/// `None` means the previous document was explicitly released and no newer
/// scan request has claimed a document yet; a coalescable job popped in that
/// window must not initialize a model for a chapter the writer already left.
fn correction_job_document_obsolete(job: &CorrectionJob) -> bool {
    let queue = correction_queue();
    let active = match queue.active_document.lock() {
        Ok(active) => active,
        Err(poisoned) => poisoned.into_inner(),
    };
    match active.as_ref() {
        None => true,
        Some((project_id, chapter_id, session_id)) => {
            project_id != &job.project_id
                || chapter_id != &job.chapter_id
                || *session_id > job.session_id
        }
    }
}

/// True when `queued` belongs to the same revision as `(session_id,
/// text_revision)` or an older one. Revisions are only comparable inside one
/// editor session, so an older session always counts as older here.
fn correction_job_is_older(queued: &CorrectionJob, session_id: u64, text_revision: u64) -> bool {
    queued.session_id < session_id
        || (queued.session_id == session_id && queued.text_revision <= text_revision)
}

/// True only for a strictly older revision. A manual whole-chapter check queued
/// for the same revision covers more than a dirty-window scan, so an equal
/// revision never justifies dropping it.
fn correction_job_is_strictly_older(
    queued: &CorrectionJob,
    session_id: u64,
    text_revision: u64,
) -> bool {
    queued.session_id < session_id
        || (queued.session_id == session_id && queued.text_revision < text_revision)
}

pub(crate) fn submit_correction_job(
    project_id: String,
    chapter_id: String,
    request_id: u32,
    session_id: u64,
    text_revision: u64,
    kind: &'static str,
    class: CorrectionJobClass,
    run: impl FnOnce() -> bool + Send + 'static,
) -> Result<(), String> {
    // restore-only 读取不算“最新待校对 revision”：它不跑模型，不能替换仍然
    // 排在队首的真实扫描（issue #1062 三.3：restore-only 不能确认完成扫描）。
    if class == CorrectionJobClass::Scan {
        record_correction_latest(&project_id, &chapter_id, session_id, text_revision);
    }
    if class != CorrectionJobClass::EditMapping {
        record_active_correction_document(&project_id, &chapter_id, session_id);
    }
    let queue = correction_queue();
    let mut jobs = match queue.jobs.lock() {
        Ok(jobs) => jobs,
        Err(_) => {
            record_correction_job_event(
                "correction.job.skipped",
                request_id,
                session_id,
                text_revision,
                kind,
                Some("error"),
                Some("queue_lock_poisoned"),
                None,
                None,
                None,
            );
            return Err("校对任务队列锁已损坏".to_string());
        }
    };
    // Issue #1061/#1062 三.3：同一章、同一编辑会话只保留最新的待校对 revision。
    // 新请求到达时就把排队中的旧扫描/旧恢复丢掉，立刻释放整章正文克隆，而不是
    // 等它排到队首再在窗口边界发现已过期；编辑映射永远按 revision 顺序执行。
    let mut coalesced = 0usize;
    if class != CorrectionJobClass::EditMapping {
        let mut retained = VecDeque::with_capacity(jobs.len());
        while let Some(queued) = jobs.pop_front() {
            let same_document = queued.project_id == project_id && queued.chapter_id == chapter_id;
            let queued_is_older = correction_job_is_older(&queued, session_id, text_revision);
            let supersedes = same_document
                && match (class, queued.class) {
                    // 新扫描返回整章快照：更旧 revision 的扫描与恢复都丢掉；
                    // 同一 revision 上它只取代排队中的脏区扫描，手动整章检查
                    // (`chapter_all`) 覆盖更广，让它自己跑完。
                    (CorrectionJobClass::Scan, CorrectionJobClass::Scan) => {
                        correction_job_is_strictly_older(&queued, session_id, text_revision)
                            || (queued_is_older && queued.kind != "chapter_all")
                    }
                    (CorrectionJobClass::Scan, CorrectionJobClass::Display) => queued_is_older,
                    // restore-only 只能取代更早的 restore-only，绝不能丢弃仍然
                    // 需要的模型扫描，否则脏区永远等不到扫描窗口。
                    (CorrectionJobClass::Display, CorrectionJobClass::Display) => queued_is_older,
                    _ => false,
                };
            if supersedes {
                coalesced += 1;
                record_correction_job_event(
                    "correction.job.coalesced",
                    queued.request_id,
                    queued.session_id,
                    queued.text_revision,
                    queued.kind,
                    Some("coalesced"),
                    Some("superseded_by_newer_request"),
                    Some(queued.queued_at.elapsed().as_millis()),
                    None,
                    None,
                );
            } else {
                retained.push_back(queued);
            }
        }
        *jobs = retained;
    }
    jobs.push_back(CorrectionJob {
        project_id,
        chapter_id,
        request_id,
        session_id,
        text_revision,
        kind,
        queued_at: Instant::now(),
        class,
        run: Box::new(run),
    });
    let queue_depth = jobs.len();
    drop(jobs);
    if coalesced > 0 {
        record_correction_job_event(
            "correction.job.coalesced",
            request_id,
            session_id,
            text_revision,
            kind,
            Some("coalesced"),
            Some("replaced_queued_requests"),
            None,
            None,
            Some(queue_depth),
        );
    }
    record_correction_job_event(
        "correction.job.queued",
        request_id,
        session_id,
        text_revision,
        kind,
        Some("queued"),
        None,
        None,
        None,
        Some(queue_depth),
    );
    queue.wake.notify_one();
    Ok(())
}

/// Drops a closed chapter's queue bookkeeping. Queued work for the closing
/// chapter is discarded so opening many chapters never grows the `latest`
/// map or leaves obsolete jobs waiting behind the shared executor.
pub(crate) fn release_correction_queue(project_id: &str, chapter_id: &str) {
    let queue = correction_queue();
    if let Ok(mut active) = queue.active_document.lock() {
        if active
            .as_ref()
            .is_some_and(|(active_project, active_chapter, _)| {
                active_project == project_id && active_chapter == chapter_id
            })
        {
            *active = None;
        }
    }
    let mut latest = match queue.latest.lock() {
        Ok(latest) => latest,
        Err(poisoned) => poisoned.into_inner(),
    };
    latest.remove(&(project_id.to_string(), chapter_id.to_string()));
    let mut jobs = match queue.jobs.lock() {
        Ok(jobs) => jobs,
        Err(poisoned) => poisoned.into_inner(),
    };
    let queue_depth = jobs.len();
    let mut retained = VecDeque::with_capacity(jobs.len());
    while let Some(job) = jobs.pop_front() {
        // Edit mappings are lightweight and keep Core's segment index in
        // revision order. Dropping one leaves a revision gap and forces the
        // next chapter check to reconcile the whole text, so they always run;
        // scan/display jobs for the closed chapter are discarded.
        if job.project_id == project_id
            && job.chapter_id == chapter_id
            && job.class != CorrectionJobClass::EditMapping
        {
            record_correction_job_event(
                "correction.job.skipped",
                job.request_id,
                job.session_id,
                job.text_revision,
                job.kind,
                Some("skipped"),
                Some("chapter_closed"),
                Some(job.queued_at.elapsed().as_millis()),
                None,
                Some(queue_depth),
            );
        } else {
            retained.push_back(job);
        }
    }
    *jobs = retained;
}
