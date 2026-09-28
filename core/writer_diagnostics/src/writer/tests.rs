//! writer.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;
use std::sync::atomic::AtomicU64;

// writer 测试共享全局单例，必须串行执行；与 export/logger 测试共用 crate 级 TEST_LOCK，
// 任一测试 panic 毒化锁后用 into_inner 恢复，避免级联 PoisonError。
fn lock() -> std::sync::MutexGuard<'static, ()> {
    crate::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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
    let _lock = lock();
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
    let _lock = lock();
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
    let _lock = lock();
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path().to_path_buf(), "test-disabled".to_string(), false);
    set_enabled(false);
    enqueue(make_event_json("test.dropped"));
    assert!(flush());
    // enabled=false 时事件被丢弃，文件不应存在
    assert!(log_files().is_empty());
    reset_for_test();
}

#[test]
fn prune_global_keeps_max_5_files() {
    let _lock = lock();
    let tmp = tempfile::tempdir().unwrap();
    let log_dir = tmp.path().join("log");
    fs::create_dir_all(&log_dir).unwrap();

    // 创建 6 个不同 build 的日志文件
    for i in 0..6 {
        let path = log_dir.join(format!("sujian-current-build{i}.log"));
        fs::write(&path, "test log content\n").unwrap();
        // 稍微间隔修改时间，确保排序正确
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let current_file = log_dir.join("sujian-current-build0.log");
    prune_old_logs_global(&log_dir, &current_file);

    let mut count = 0;
    for entry in fs::read_dir(&log_dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(LOG_PREFIX) && name.ends_with(".log") {
            count += 1;
        }
    }
    assert_eq!(
        count, MAX_TOTAL_LOG_FILES,
        "should keep at most {} files, got {}",
        MAX_TOTAL_LOG_FILES, count
    );
    reset_for_test();
}

#[test]
fn rotate_then_prune_keeps_max_5_files() {
    let _lock = lock();
    let tmp = tempfile::tempdir().unwrap();
    let log_dir = tmp.path().join("log");
    fs::create_dir_all(&log_dir).unwrap();

    // 先创建 4 个旧的轮转日志（不同 build）
    for i in 0..4 {
        let path = log_dir.join(format!("sujian-current-build{i}.log"));
        fs::write(&path, "old log content\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // 创建当前 build 的日志文件，大小超过 1 MiB
    let current_file = log_dir.join("sujian-current-currentbuild.log");
    let big_content = "x".repeat(MAX_FILE_SIZE as usize + 1);
    fs::write(&current_file, &big_content).unwrap();
    // 确保当前文件修改时间最新
    std::thread::sleep(std::time::Duration::from_millis(10));

    // 触发轮转：rotate_if_needed 会把 current 改名，然后 prune
    assert!(
        rotate_if_needed(&log_dir, &current_file),
        "rotate should succeed"
    );

    // 创建新的 current 文件
    fs::write(&current_file, "new current\n").unwrap();

    // 统计文件数
    let mut count = 0;
    for entry in fs::read_dir(&log_dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(LOG_PREFIX) && name.ends_with(".log") {
            count += 1;
        }
    }
    assert_eq!(
        count, MAX_TOTAL_LOG_FILES,
        "after rotate+prune+new current, should have exactly {} files, got {}",
        MAX_TOTAL_LOG_FILES, count
    );
    reset_for_test();
}

/// 验证 Issue #682 评论 5658905879 指出的边界 bug 已修复：
/// 新 build 首次写入时 current_file 不存在，修复前 prune_old_logs_global 用
/// `current_file.exists()` 决定预留位置导致 reserve_for_current=0，5 个旧 build
/// 日志全部保留；随后 OpenOptions::create(true) 再创建新 current，目录变成 6 个
/// 文件，超过 MAX_TOTAL_LOG_FILES=5。修复后 prune_old_logs_global 固定给 current
/// 预留 1 个名额，新 build 首次写入后目录最多保留 MAX_TOTAL_LOG_FILES 个文件。
///
/// 修复后断言：count <= MAX_TOTAL_LOG_FILES 且新 build 的 current 文件存在。
#[test]
fn new_build_first_write_keeps_max_5_files() {
    let _lock = lock();
    let tmp = tempfile::tempdir().unwrap();
    let log_dir = tmp.path().join("log");
    fs::create_dir_all(&log_dir).unwrap();

    // 先在目录里放 5 个旧 build 的日志文件，间隔 10ms 修改时间确保排序正确。
    for i in 0..5 {
        let path = log_dir.join(format!("sujian-current-old{i}.log"));
        fs::write(&path, "old build log content\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // 用一个目录里从未存在过的新 build key 走 init/enqueue/flush。
    init(log_dir.clone(), "newbuild-repro".to_string(), true);
    set_enabled(true);
    enqueue(make_event_json("repro.event"));
    assert!(flush(), "flush should succeed");

    // 统计整个 log_dir 目录下所有 sujian-current*.log 文件数量。
    let mut count = 0;
    for entry in fs::read_dir(&log_dir).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(LOG_PREFIX) && name.ends_with(".log") {
            count += 1;
        }
    }

    // 修复后断言：目录文件数不超过 MAX_TOTAL_LOG_FILES。
    assert!(
        count <= MAX_TOTAL_LOG_FILES,
        "修复后新 build 首次写入目录文件数应 <= MAX_TOTAL_LOG_FILES={}, 实际 {}",
        MAX_TOTAL_LOG_FILES,
        count
    );

    // 同时断言新 build 的 current 文件存在。
    let new_current = log_dir.join("sujian-current-newbuild-repro.log");
    assert!(
        new_current.exists(),
        "新 build 的 current 文件应存在: {new_current:?}"
    );

    reset_for_test();
}
