//! 后台写入队列、顺序、轮转、flush/clear barrier。
//!
//! 把 Android `PersistentLogWriter.kt` 的有序队列/屏障语义搬到 Rust：
//! - 单一 writer 线程独占文件 I/O
//! - 命令队列：Append / FlushBarrier / ClearBarrier
//! - 1 MiB / 5 文件轮转（4 轮转 + 1 当前）
//! - flush/clear 返回 Boolean 表示成功
//! - 落盘健康位
//! - 日志文件名：`sujian-current-{build_key}.log`，build_key 从 init 传入
//!
//! 线程安全模型：`Mutex<VecDeque<Command>>` + `Condvar` 守护队列；
//! 文件 I/O 在 lock 外只由 writer 线程执行，故 enqueue 与 flush 不会阻塞 I/O。
//!
//! 顺序不变量：barrier 入队前的所有 Append 先于 barrier 处理，barrier 后的 Append
//! 后于 barrier 处理——天然保证 flush 等到前序日志落盘、clear 在 writer 空闲后
//! 删除文件且后续 Append 不会写回旧日志。

use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 日志文件前缀。
const LOG_PREFIX: &str = "sujian-current";
/// 单文件大小上限：1 MiB。
const MAX_FILE_SIZE: u64 = 1024 * 1024;
/// 同一构建下日志文件总数上限（当前 + 轮转）。
const MAX_TOTAL_FILES_PER_BUILD: usize = 5;
/// 同一构建下轮转文件上限（不含当前文件）。
const MAX_ROTATED_FILES_PER_BUILD: usize = MAX_TOTAL_FILES_PER_BUILD - 1;
/// flush/clear 等待上限：writer 死亡时调用方不能永久挂起。
const BARRIER_TIMEOUT_MS: u64 = 5_000;

/// writer 线程处理的命令。单一有序队列保证：旧日志 → flush/clear barrier → 新日志
/// 的全局顺序，无需计数器同步。
enum Command {
    /// 追加一条事件。
    Append(String),
    /// flush 屏障：writer 处理到此命令时先写完前序 Append，再通知等待者。
    /// `AtomicBool` 是 writer → 调用方的结果位：true 表示前序日志确实落盘。
    FlushBarrier(Arc<AtomicBool>, Arc<AtomicBool>),
    /// clear 屏障：writer 处理到此命令时先写完前序 Append，再删除日志目录所有文件，
    /// 最后通知等待者。`AtomicBool` 是删除结果位。
    ClearBarrier(Arc<AtomicBool>, Arc<AtomicBool>),
}

/// 全局 writer 单例。
struct WriterState {
    queue: Mutex<VecDeque<Command>>,
    condvar: Condvar,
    /// writer 线程句柄，仅在 init 时设置。
    thread: OnceLock<JoinHandle<()>>,
    /// 是否已初始化。
    initialized: AtomicBool,
    /// 是否启用（可动态切换）。
    enabled: AtomicBool,
    /// 日志目录 + build_key，由 init 设置。
    config: Mutex<Option<WriterConfig>>,
}

struct WriterConfig {
    log_dir: PathBuf,
    build_key: String,
}

impl Clone for WriterConfig {
    fn clone(&self) -> Self {
        Self {
            log_dir: self.log_dir.clone(),
            build_key: self.build_key.clone(),
        }
    }
}

static STATE: OnceLock<WriterState> = OnceLock::new();

fn state() -> &'static WriterState {
    STATE.get_or_init(|| WriterState {
        queue: Mutex::new(VecDeque::new()),
        condvar: Condvar::new(),
        thread: OnceLock::new(),
        initialized: AtomicBool::new(false),
        enabled: AtomicBool::new(false),
        config: Mutex::new(None),
    })
}

/// 初始化 writer 线程。幂等：重复调用无副作用。
///
/// `log_dir` 是日志目录，`build_key` 用于日志文件名（`sujian-current-{build_key}.log`）。
/// 目录创建推迟到 writer 线程第一次写盘，本函数不做任何文件 I/O。
pub(crate) fn init(log_dir: PathBuf, build_key: String, enabled: bool) {
    let s = state();
    if s.initialized.swap(true, Ordering::SeqCst) {
        // 已初始化：只更新 config 和 enabled。
        if let Ok(mut cfg) = s.config.lock() {
            *cfg = Some(WriterConfig { log_dir, build_key });
        }
        s.enabled.store(enabled, Ordering::SeqCst);
        return;
    }
    if let Ok(mut cfg) = s.config.lock() {
        *cfg = Some(WriterConfig { log_dir, build_key });
    }
    s.enabled.store(enabled, Ordering::SeqCst);
    let handle = std::thread::Builder::new()
        .name("sujian-diagnostics-writer".to_string())
        .spawn(writer_loop)
        .ok();
    if let Some(h) = handle {
        let _ = s.thread.set(h);
    }
}

/// 设置 enabled 标志位（writer 线程读取）。
pub(crate) fn set_enabled(enabled: bool) {
    state().enabled.store(enabled, Ordering::SeqCst);
}

/// 把一条 JSONL 事件入队。非阻塞。
/// 未初始化或被禁用时直接丢弃。
pub(crate) fn enqueue(event_json: String) {
    let s = state();
    if !s.initialized.load(Ordering::SeqCst) || !s.enabled.load(Ordering::SeqCst) {
        return;
    }
    if let Ok(mut q) = s.queue.lock() {
        q.push_back(Command::Append(event_json));
        s.condvar.notify_one();
    }
}

/// 阻塞直到调用前所有已 enqueue 的事件都被 writer 写完落盘。
///
/// 返回 `true` 表示落盘成功；writer 死亡超时或写盘失败返回 `false`。
/// 未初始化时返回 `true`（没有可 flush 的日志）。
pub(crate) fn flush() -> bool {
    let s = state();
    if !s.initialized.load(Ordering::SeqCst) {
        return true;
    }
    let done = Arc::new(AtomicBool::new(false));
    let persisted = Arc::new(AtomicBool::new(false));
    if let Ok(mut q) = s.queue.lock() {
        q.push_back(Command::FlushBarrier(done.clone(), persisted.clone()));
        s.condvar.notify_one();
    }
    wait_for_barrier(&done, BARRIER_TIMEOUT_MS) && persisted.load(Ordering::SeqCst)
}

/// 清空日志：入队 ClearBarrier，writer 处理到此命令时先写完前序 Append，
/// 再删除日志目录下所有文件，最后通知等待者。
///
/// 返回 `true` 表示删除成功；超时/中断/删除失败返回 `false`。
/// 未初始化时返回 `true`。
pub(crate) fn clear() -> bool {
    let s = state();
    if !s.initialized.load(Ordering::SeqCst) {
        return true;
    }
    let done = Arc::new(AtomicBool::new(false));
    let deleted = Arc::new(AtomicBool::new(false));
    if let Ok(mut q) = s.queue.lock() {
        q.push_back(Command::ClearBarrier(done.clone(), deleted.clone()));
        s.condvar.notify_one();
    }
    wait_for_barrier(&done, BARRIER_TIMEOUT_MS) && deleted.load(Ordering::SeqCst)
}

/// 等待 barrier 完成，最多等待 `timeout_ms` 毫秒。
fn wait_for_barrier(done: &Arc<AtomicBool>, timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while !done.load(Ordering::SeqCst) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// writer 线程主循环：wait → drain 整个命令队列 → 按序处理命令 → 写盘/删文件。
fn writer_loop() {
    let s = state();
    let mut persistence_healthy = true;
    loop {
        let commands: Vec<Command> = {
            let mut q = match s.queue.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            while q.is_empty() {
                q = match s.condvar.wait(q) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            q.drain(..).collect()
        };
        process_commands(&commands, &mut persistence_healthy);
    }
}

/// 在 lock 外按入队顺序处理命令。连续 Append 收集为 batch 写盘；
/// FlushBarrier/ClearBarrier 的 done 在 try/finally 中必定释放。
fn process_commands(commands: &[Command], persistence_healthy: &mut bool) {
    let s = state();
    let mut batch: Vec<String> = Vec::new();
    for cmd in commands.iter() {
        match cmd {
            Command::Append(json) => batch.push(json.clone()),
            Command::FlushBarrier(done, persisted) => {
                let batch_ok = flush_batch(s, &batch);
                batch.clear();
                *persistence_healthy = *persistence_healthy && batch_ok;
                persisted.store(*persistence_healthy, Ordering::SeqCst);
                done.store(true, Ordering::SeqCst);
            }
            Command::ClearBarrier(done, deleted) => {
                let _ = flush_batch(s, &batch);
                batch.clear();
                let deleted_ok = clear_log_files(s);
                if deleted_ok {
                    *persistence_healthy = true;
                }
                deleted.store(deleted_ok, Ordering::SeqCst);
                done.store(true, Ordering::SeqCst);
            }
        }
    }
    // 尾部连续 Append 写盘：尾部 batch 写失败要更新 persistence_healthy。
    if !batch.is_empty() {
        let tail_ok = flush_batch(s, &batch);
        if !tail_ok {
            *persistence_healthy = false;
        }
    }
}

/// 把当前累积的 batch 写盘并清空（仅由 writer 线程调用）。
/// 返回写盘成功与否；空 batch 返回 true。
fn flush_batch(s: &'static WriterState, batch: &[String]) -> bool {
    if batch.is_empty() {
        return true;
    }
    write_batch(s, batch)
}

/// 把一整批事件 append 到当前日志文件，每个 batch 写完即 flush。
fn write_batch(s: &'static WriterState, batch: &[String]) -> bool {
    let cfg = match s.config.lock() {
        Ok(g) => g.clone(),
        Err(_) => return false,
    };
    let Some(cfg) = cfg else {
        return false;
    };
    if fs::create_dir_all(&cfg.log_dir).is_err() {
        return false;
    }
    let current_file = current_log_path(&cfg.log_dir, &cfg.build_key);
    if !rotate_if_needed(&cfg.log_dir, &current_file, &cfg.build_key) {
        return false;
    }
    let mut file = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&current_file)
    {
        Ok(f) => f,
        Err(_) => return false,
    };
    for line in batch {
        if file.write_all(line.as_bytes()).is_err() {
            return false;
        }
        if file.write_all(b"\n").is_err() {
            return false;
        }
    }
    if file.flush().is_err() {
        return false;
    }
    true
}

/// 当前文件超过 1 MiB 时移动到带时间戳的轮转文件，并裁剪到当前 buildKey 的
/// `MAX_ROTATED_FILES_PER_BUILD` 个轮转文件。
fn rotate_if_needed(log_dir: &Path, current_file: &Path, build_key: &str) -> bool {
    let metadata = match fs::metadata(current_file) {
        Ok(m) => m,
        Err(_) => return true, // 文件不存在，无需轮转
    };
    if metadata.len() < MAX_FILE_SIZE {
        return true;
    }
    let base_name = current_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(LOG_PREFIX);
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
    let rotated = log_dir.join(format!("{base_name}-{ts}.log"));
    if fs::rename(current_file, &rotated).is_err() {
        return false;
    }
    prune_old_logs(log_dir, build_key)
}

/// 按最后修改时间降序保留当前 buildKey 的前 `MAX_ROTATED_FILES_PER_BUILD` 个轮转文件，
/// 多余删除。当前文件不参与 rotated 计数。
fn prune_old_logs(log_dir: &Path, build_key: &str) -> bool {
    let current_name = format!("{LOG_PREFIX}-{build_key}.log");
    let prefix = format!("{LOG_PREFIX}-{build_key}");
    let mut rotated: Vec<PathBuf> = Vec::new();
    let entries = match fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(_) => return true, // 目录不可读，视为无多余文件
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(&prefix) && name_str.ends_with(".log") && name_str != current_name {
            rotated.push(entry.path());
        }
    }
    // 按修改时间降序排序。
    rotated.sort_by(|a, b| {
        let ma = fs::metadata(a).and_then(|m| m.modified()).ok();
        let mb = fs::metadata(b).and_then(|m| m.modified()).ok();
        mb.cmp(&ma)
    });
    let mut all_deleted = true;
    for path in rotated.iter().skip(MAX_ROTATED_FILES_PER_BUILD) {
        if fs::remove_file(path).is_err() {
            all_deleted = false;
        }
    }
    all_deleted
}

/// 删除日志目录下的所有文件（仅由 writer 线程调用）。
/// 只删除文件；子目录及其内容属于未知数据，绝不触碰。
fn clear_log_files(s: &'static WriterState) -> bool {
    let cfg = match s.config.lock() {
        Ok(g) => g.clone(),
        Err(_) => return false,
    };
    let Some(cfg) = cfg else {
        return false;
    };
    let entries = match fs::read_dir(&cfg.log_dir) {
        Ok(e) => e,
        Err(_) => return true, // 目录不存在视为无可删内容
    };
    let mut all_deleted = true;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && fs::remove_file(&path).is_err() {
            all_deleted = false;
        }
    }
    all_deleted
}

/// 当前日志文件路径：`{log_dir}/sujian-current-{build_key}.log`。
fn current_log_path(log_dir: &Path, build_key: &str) -> PathBuf {
    log_dir.join(format!("{LOG_PREFIX}-{build_key}.log"))
}

/// 返回当前日志目录下所有 `sujian-current*.log` 文件路径（供 export 复制）。
pub(crate) fn log_files() -> Vec<PathBuf> {
    let s = state();
    let cfg = match s.config.lock() {
        Ok(g) => g.clone(),
        Err(_) => return Vec::new(),
    };
    let Some(cfg) = cfg else {
        return Vec::new();
    };
    let entries = match fs::read_dir(&cfg.log_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(LOG_PREFIX) && name_str.ends_with(".log") {
            files.push(entry.path());
        }
    }
    files.sort();
    files
}

/// 仅用于测试：把单例恢复到干净状态（清空队列、删除日志文件）。
#[cfg(test)]
pub(crate) fn reset_for_test() {
    let s = state();
    if let Ok(mut q) = s.queue.lock() {
        q.clear();
    }
    let log_dir = s
        .config
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|c| c.log_dir.clone()));
    let Some(log_dir) = log_dir else { return };
    let Ok(entries) = fs::read_dir(&log_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    // writer 测试共享全局单例，必须串行执行。用 mutex 保证同一时刻只有一个测试在跑。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn next_seq() -> u64 {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        SEQ.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn make_event_json(event: &str) -> String {
        format!(
            r#"{{"ts":0,"seq":{},"level":"INFO","origin":"app","event":"{}","target":"test","session":"s"}}"#,
            next_seq(),
            event
        )
    }

    #[test]
    fn enqueue_and_flush_writes_file() {
        let _lock = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        init(tmp.path().to_path_buf(), "test-build".to_string(), true);
        set_enabled(true);
        enqueue(make_event_json("test.event1"));
        enqueue(make_event_json("test.event2"));
        assert!(flush(), "flush should succeed");
        let files = log_files();
        assert!(!files.is_empty(), "log files should exist: {files:?}");
        let content = fs::read_to_string(&files[0]).unwrap();
        assert!(content.contains("test.event1"), "content: {content}");
        assert!(content.contains("test.event2"), "content: {content}");
        reset_for_test();
    }

    #[test]
    fn clear_removes_files() {
        let _lock = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        init(
            tmp.path().to_path_buf(),
            "test-build-clear".to_string(),
            true,
        );
        set_enabled(true);
        enqueue(make_event_json("test.event"));
        assert!(flush());
        assert!(!log_files().is_empty());
        assert!(clear(), "clear should succeed");
        assert!(log_files().is_empty(), "files should be cleared");
        reset_for_test();
    }

    #[test]
    fn disabled_drops_events() {
        let _lock = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        init(tmp.path().to_path_buf(), "test-disabled".to_string(), false);
        set_enabled(false);
        enqueue(make_event_json("test.dropped"));
        assert!(flush());
        // enabled=false 时事件被丢弃，文件不应存在
        assert!(log_files().is_empty());
        reset_for_test();
    }
}
