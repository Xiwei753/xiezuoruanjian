// =============================================================================
// diagnostics.rs — Linux_qt 平台诊断采集器
// =============================================================================
//
// 引用了什么：
// - std::sync::OnceLock：平台信息缓存、资源采样线程幂等标记
// - std::collections::BTreeMap / std::time::{Duration, Instant}：进程资源采样
//
// 干什么的：
// - 检测 Linux 平台特有的 AppImage 运行时包类型，计算有效 buildKey
// - 收集 Qt 运行时信息、系统信息（由 main.rs 注入），供导出诊断包时作为
//   PlatformAttachment 交给 Rust 统一诊断后端 (writer_diagnostics)
// - 后台采样进程资源（RSS/PSS/线程数/进程与线程 CPU），发 process.resource
//   诊断事件（Issue #1061 第 6 项）
//
// 不干什么：
// - 不负责日志写入、轮转、脱敏、落盘（由 writer_diagnostics 接管）
// - 不负责诊断配置（由 writer_diagnostics::set_config 接管）
// - 不负责诊断包导出/打包（由 writer_diagnostics::export 接管）
// - 不负责 panic hook（由 writer_diagnostics::init 中的 install_panic_hook 接管，
//   Issue #670 评论 5651816143 修改 6）
//
// 被什么引用：
// - main.rs：init_build_identity / set_qt_version / set_runtime_info / set_system_info
// - main.rs：start_process_resource_monitor（诊断后端初始化后启动采样线程）
// - settings_backend.rs：get_runtime_info / get_system_info（构造导出附件）
// - app_backend.rs：effective_build_key（日志文件名）
// =============================================================================

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// 编译时 buildKey，由 build.rs 注入
///
/// 注意：这是编译期固化的回退值。运行时应通过 `init_build_identity()` 初始化
/// `EFFECTIVE_BUILD_KEY`，并通过 `effective_build_key()` 获取反映运行时真实包类型
/// （如 AppImage）的有效值。日志文件名和 manifest buildKey 字段统一使用
/// `effective_build_key()`，避免编译期/运行期语义冲突（Issue #665）。
const BUILD_KEY: &str = env!("BUILD_KEY");
/// 编译时 packageType，由 build.rs 注入
///
/// 同样是编译期回退值，运行时有效值通过 `effective_package_type()` 获取。
const PACKAGE_TYPE: &str = env!("PACKAGE_TYPE");

/// 运行时有效 buildKey 缓存，由 `init_build_identity()` 设置
///
/// AppImage 分发场景下，编译期 BUILD_KEY 嵌入的是编译时 packageType（默认 dev），
/// 而运行时 APPIMAGE 环境变量存在，真实包类型为 AppImage。此缓存存储运行时计算
/// 得到的有效 buildKey，确保日志文件名和 manifest buildKey 字段反映运行时真实包类型。
static EFFECTIVE_BUILD_KEY: OnceLock<String> = OnceLock::new();

/// 运行时有效 packageType 缓存，由 `init_build_identity()` 设置
///
/// APPIMAGE 环境变量存在时为 "AppImage"，否则为编译期 PACKAGE_TYPE。
static EFFECTIVE_PACKAGE_TYPE: OnceLock<String> = OnceLock::new();

/// 初始化运行时有效 build identity
///
/// 在 `main()` 最早期调用，确保所有日志写入都使用有效 build key。
///
/// 规则：
/// - 若 `APPIMAGE` 环境变量存在，有效 package type = `"AppImage"`
/// - 否则有效 package type = 编译期 `PACKAGE_TYPE`
/// - 若有效 package type == 编译期 PACKAGE_TYPE，有效 build key = 编译期 BUILD_KEY
/// - 否则重新组合：`{version}-{gitSha}-{effectivePackageType}-{buildProfile}`
///
/// 幂等：多次调用安全，仅首次调用生效（OnceLock 语义）。
pub fn init_build_identity() {
    let (effective_package_type, effective_build_key) = compute_effective_build_identity();
    let _ = EFFECTIVE_PACKAGE_TYPE.set(effective_package_type);
    let _ = EFFECTIVE_BUILD_KEY.set(effective_build_key);
}

/// 计算运行时有效 build identity（纯函数，便于测试）
///
/// 返回 `(effective_package_type, effective_build_key)`。
/// 不读取或写入全局 OnceLock，仅依据当前进程环境变量和编译期常量计算。
///
/// - `appimage_present`: APPIMAGE 环境变量是否存在
fn compute_effective_build_identity_with(appimage_present: bool) -> (String, String) {
    let effective_package_type = if appimage_present {
        "AppImage".to_string()
    } else {
        PACKAGE_TYPE.to_string()
    };

    let effective_build_key = if effective_package_type == PACKAGE_TYPE {
        BUILD_KEY.to_string()
    } else {
        format!(
            "{}-{}-{}-{}",
            env!("CARGO_PKG_VERSION"),
            env!("GIT_COMMIT_SHA"),
            effective_package_type,
            env!("BUILD_PROFILE")
        )
    };

    (effective_package_type, effective_build_key)
}

/// 计算运行时有效 build identity（读取当前进程环境变量）
///
/// 返回 `(effective_package_type, effective_build_key)`。
/// 不读取或写入全局 OnceLock，仅依据当前进程环境变量和编译期常量计算。
fn compute_effective_build_identity() -> (String, String) {
    compute_effective_build_identity_with(std::env::var_os("APPIMAGE").is_some())
}

/// 获取运行时有效 buildKey
///
/// 返回 `init_build_identity()` 设置的缓存值；未初始化时回退到编译期 BUILD_KEY。
/// 日志文件名和 manifest buildKey 字段应统一使用此函数，确保反映运行时真实包类型。
pub fn effective_build_key() -> &'static str {
    EFFECTIVE_BUILD_KEY
        .get()
        .map(|s| s.as_str())
        .unwrap_or(BUILD_KEY)
}

/// 获取运行时有效 packageType
///
/// 返回 `init_build_identity()` 设置的缓存值；未初始化时回退到编译期 PACKAGE_TYPE。
/// `collect_runtime_info()` 应使用此函数填充 RuntimeInfo.package_type，确保与
/// bundled_qt（运行时 APPIMAGE 检测）语义一致。
pub fn effective_package_type() -> &'static str {
    EFFECTIVE_PACKAGE_TYPE
        .get()
        .map(|s| s.as_str())
        .unwrap_or(PACKAGE_TYPE)
}

// ── 平台信息采集器 ──
//
// 以下结构体和缓存用于收集 Linux 平台特有的运行时信息（Qt 版本、QSysInfo 等），
// 供导出诊断包时作为 PlatformAttachment 交给 Rust 统一诊断后端。
// 这些只是 OnceLock 的 set/get，不涉及文件 I/O、脱敏、轮转或落盘。

/// 全局 Qt 版本缓存，由 main.rs 通过 set_qt_version() 注入
static QT_VERSION: OnceLock<String> = OnceLock::new();

/// 全局 RuntimeInfo 缓存，由 main.rs 通过 set_runtime_info() 注入
static RUNTIME_INFO: OnceLock<RuntimeInfo> = OnceLock::new();

/// 全局 SystemInfo 缓存，由 main.rs 通过 set_system_info() 注入
static SYSTEM_INFO: OnceLock<SystemInfo> = OnceLock::new();

/// 设置 Qt 版本（由 main.rs 在启动时调用，使用 cpp! 宏获取运行时版本）
pub fn set_qt_version(version: &str) {
    let _ = QT_VERSION.set(version.to_string());
}

/// 设置 RuntimeInfo（由 main.rs 在启动时调用）
pub fn set_runtime_info(info: RuntimeInfo) {
    let _ = RUNTIME_INFO.set(info);
}

/// 设置 SystemInfo（由 main.rs 在启动时调用）
pub fn set_system_info(info: SystemInfo) {
    let _ = SYSTEM_INFO.set(info);
}

/// 获取 RuntimeInfo（由 settings_backend.rs 调用 export 时构造附件使用）
pub fn get_runtime_info() -> Option<&'static RuntimeInfo> {
    RUNTIME_INFO.get()
}

/// 获取 SystemInfo（由 settings_backend.rs 调用 export 时构造附件使用）
pub fn get_system_info() -> Option<&'static SystemInfo> {
    SYSTEM_INFO.get()
}

/// 获取 Qt 版本（由 settings_backend.rs 构造 device_info 附件使用）
pub fn get_qt_version() -> Option<&'static str> {
    QT_VERSION.get().map(|s| s.as_str())
}

/// Qt 运行时信息，由 main.rs 从 DesktopRuntimeProfile 转换而来
pub struct RuntimeInfo {
    pub qt_runtime_version: String,
    pub qt_build_version: String,
    pub qpa_platform: String,
    pub input_method_module: String,
    pub bundled_qt: bool,
    pub package_type: String,
    pub rustc_version: String,
}

/// 系统信息，由 main.rs 通过 QSysInfo 收集
pub struct SystemInfo {
    pub product_type: String,
    pub product_version: String,
    pub pretty_product_name: String,
    pub kernel_type: String,
    pub kernel_version: String,
    pub current_cpu_arch: String,
    pub build_abi: String,
    pub xdg_current_desktop: String,
    pub xdg_session_type: String,
}

// ── 进程资源采样（Issue #1061 第 6 项）──
//
// 轻量后台线程按固定间隔读取 /proc/self 的资源统计（内存、线程数、进程/线程 CPU），
// 以 event=process.resource 交给 writer_diagnostics 统一落盘。
// 只读 /proc 的统计字段，不读取正文或任何用户数据；所有读取失败都静默跳过。

/// 资源采样间隔（毫秒字段用同一常量换算）
const RESOURCE_SAMPLE_INTERVAL: Duration = Duration::from_secs(30);

/// 首个样本的延迟：给启动阶段留出稳定时间
const RESOURCE_SAMPLE_INITIAL_DELAY: Duration = Duration::from_secs(5);

/// Linux 惯例 USER_HZ=100：/proc 的 utime/stime 以 1/100 秒（tick）计时。
const PROC_USER_HZ: f64 = 100.0;

/// top_threads 最多上报的线程数
const TOP_THREADS_LIMIT: usize = 3;

/// 资源采样线程启动标记（OnceLock 保证只启动一次）
static RESOURCE_MONITOR_STARTED: OnceLock<()> = OnceLock::new();

/// 一行 /proc/<pid>/stat 中本模块关心的字段。
///
/// comm 位于首个 '(' 与最后一个 ')' 之间（可能包含空格或括号）；其后字段从
/// state 开始。utime/stime 是整体第 14/15 字段，即 comm 之后 tokens[11]/tokens[12]。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcStat {
    /// 进程/线程名（comm），可能包含空格
    name: String,
    /// 用户态累计 tick
    utime_ticks: u64,
    /// 内核态累计 tick
    stime_ticks: u64,
}

impl ProcStat {
    /// 累计 CPU tick（USER_HZ 单位）
    fn cpu_ticks(&self) -> u64 {
        self.utime_ticks.saturating_add(self.stime_ticks)
    }
}

/// 解析一行 /proc/<pid>/stat（纯函数，不读真实 /proc，便于单测）。
///
/// 格式不合法时返回 None，调用方静默跳过。
fn parse_proc_stat(line: &str) -> Option<ProcStat> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close <= open {
        return None;
    }
    let name = line.get(open + 1..close)?.to_string();
    // comm 之后第一个字段是 state（整体第 3 字段），utime/stime 是整体第 14/15 字段。
    let rest = line.get(close + 1..)?;
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let utime_ticks = tokens.get(11)?.parse::<u64>().ok()?;
    let stime_ticks = tokens.get(12)?.parse::<u64>().ok()?;
    Some(ProcStat {
        name,
        utime_ticks,
        stime_ticks,
    })
}

/// /proc/self/status 中与内存/线程相关的快照
struct ProcStatusSample {
    rss_kb: Option<u64>,
    vm_size_kb: Option<u64>,
    threads: Option<u64>,
}

/// 从 /proc 的 `Key: value unit` 行里取第一个数值（kB 或计数）。
fn read_status_value(content: &str, key: &str) -> Option<u64> {
    content.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim() != key {
            return None;
        }
        value.split_whitespace().next()?.parse::<u64>().ok()
    })
}

/// 读取 /proc/self/status 的 VmRSS / VmSize / Threads。
fn read_proc_status() -> Option<ProcStatusSample> {
    let content = std::fs::read_to_string("/proc/self/status").ok()?;
    Some(ProcStatusSample {
        rss_kb: read_status_value(&content, "VmRSS"),
        vm_size_kb: read_status_value(&content, "VmSize"),
        threads: read_status_value(&content, "Threads"),
    })
}

/// 读取 /proc/self/smaps_rollup 的 `Pss:`（kB）。
///
/// 文件不存在（旧内核/受限环境）或读取失败都返回 None，只跳过该字段。
fn read_pss_kb() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
    read_status_value(&content, "Pss")
}

/// 读取 /proc/self/stat 的累计 CPU tick；失败返回 None。
fn read_self_cpu_ticks() -> Option<u64> {
    read_stat_ticks("/proc/self/stat")
}

fn read_stat_ticks(path: &str) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_proc_stat(&content).map(|stat| stat.cpu_ticks())
}

/// 读取 /proc/self/task 下所有线程的累计 CPU tick。
///
/// 返回 `(任务目录条数, 线程统计列表)`；目录读取失败返回 None，调用方静默跳过。
/// 只读每个线程的 stat 行，不读线程栈或 smaps 明细。
fn read_thread_stat_samples() -> Option<(usize, Vec<(u64, ProcStat)>)> {
    let mut thread_count = 0usize;
    let mut samples = Vec::new();
    for entry in std::fs::read_dir("/proc/self/task").ok()?.flatten() {
        thread_count += 1;
        let Some(tid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u64>().ok())
        else {
            continue;
        };
        let stat_path = entry.path().join("stat");
        let Some(stat) = std::fs::read_to_string(stat_path)
            .ok()
            .and_then(|content| parse_proc_stat(&content))
        else {
            continue;
        };
        samples.push((tid, stat));
    }
    Some((thread_count, samples))
}

/// 启动进程资源采样线程（幂等）
///
/// 线程名 `process-resource-monitor`：首个样本在启动 5 秒后采集，此后每 30 秒
/// 发一条 `process.resource` 诊断事件（RSS/PSS/线程数/进程 CPU 与 top 3 线程）。
///
/// 重复调用直接返回；spawn 失败只 warn，不 panic，也不影响启动流程。
pub fn start_process_resource_monitor() {
    if RESOURCE_MONITOR_STARTED.set(()).is_err() {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("process-resource-monitor".to_string())
        .spawn(run_process_resource_monitor)
    {
        log::warn!(target: "diagnostics", "process_resource_monitor_spawn_failed: {error}");
    }
}

/// 采样线程主体：首样本延迟 5 秒，此后每 30 秒一条 process.resource 事件。
fn run_process_resource_monitor() {
    let mut baseline: Option<(u64, Instant)> = None;
    let mut previous_thread_ticks: std::collections::HashMap<u64, u64> =
        std::collections::HashMap::new();
    let mut first_sample = true;
    loop {
        std::thread::sleep(if first_sample {
            RESOURCE_SAMPLE_INITIAL_DELAY
        } else {
            RESOURCE_SAMPLE_INTERVAL
        });
        first_sample = false;

        // status 是整个样本的基础；读不到（非 Linux 或受限环境）就跳过本轮。
        let Some(status) = read_proc_status() else {
            continue;
        };
        let cpu_ticks = read_self_cpu_ticks();
        let thread_samples = read_thread_stat_samples();
        let pss_kb = read_pss_kb();

        let mut fields: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        if let Some(rss_kb) = status.rss_kb {
            fields.insert("rss_kb".to_string(), serde_json::Value::from(rss_kb));
        }
        if let Some(pss_kb) = pss_kb {
            fields.insert("pss_kb".to_string(), serde_json::Value::from(pss_kb));
        }
        if let Some(vm_size_kb) = status.vm_size_kb {
            fields.insert(
                "vm_size_kb".to_string(),
                serde_json::Value::from(vm_size_kb),
            );
        }
        let thread_count = thread_samples
            .as_ref()
            .map(|(count, _)| *count)
            .or_else(|| status.threads.and_then(|count| usize::try_from(count).ok()));
        if let Some(thread_count) = thread_count {
            fields.insert(
                "thread_count".to_string(),
                serde_json::Value::from(thread_count),
            );
        }
        // Issue #1061 复审：行纹理不再是单个 HashMap 的模糊占用，而是分成
        // 缓存独占、快照共享和所有 LineImage 所有者的存活字节。
        let texture = crate::sujian_editor_item::line_snapshot::texture_memory_metrics();
        fields.insert(
            "texture_cache_only_bytes".to_string(),
            serde_json::Value::from(texture.cache_only_bytes),
        );
        fields.insert(
            "texture_shared_bytes".to_string(),
            serde_json::Value::from(texture.shared_bytes),
        );
        fields.insert(
            "texture_live_bytes".to_string(),
            serde_json::Value::from(texture.live_bytes),
        );
        fields.insert(
            "texture_lines".to_string(),
            serde_json::Value::from(texture.lines),
        );
        fields.insert(
            "active_visual_snapshots".to_string(),
            serde_json::Value::from(texture.active_visual_snapshots),
        );

        // 进程 CPU：两次采样的 tick 差 / 墙钟差。
        // 第一次采样只建立基线，cpu_percent=0 并标 baseline=true。
        let mut wall_ms = 0u64;
        let mut cpu_percent = 0.0f64;
        let mut ticks_delta = 0u64;
        let mut baseline_only = true;
        if let (Some((prev_ticks, prev_instant)), Some(now_ticks)) = (baseline, cpu_ticks) {
            let elapsed = prev_instant.elapsed();
            let elapsed_secs = elapsed.as_secs_f64();
            ticks_delta = now_ticks.saturating_sub(prev_ticks);
            wall_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
            if elapsed_secs > 0.0 {
                // Linux 惯例 USER_HZ=100：1 tick = 10ms；tick 差先换算成秒再除以墙钟秒数。
                cpu_percent = ticks_delta as f64 / PROC_USER_HZ / elapsed_secs * 100.0;
                baseline_only = false;
            }
        }
        if let Some(now_ticks) = cpu_ticks {
            baseline = Some((now_ticks, Instant::now()));
        }
        fields.insert(
            "cpu_percent".to_string(),
            serde_json::Value::from(cpu_percent),
        );
        fields.insert(
            "cpu_ticks_delta".to_string(),
            serde_json::Value::from(ticks_delta),
        );
        fields.insert("wall_ms".to_string(), serde_json::Value::from(wall_ms));
        fields.insert(
            "sample_interval_ms".to_string(),
            serde_json::Value::from(
                u64::try_from(RESOURCE_SAMPLE_INTERVAL.as_millis()).unwrap_or_default(),
            ),
        );
        if baseline_only {
            fields.insert("baseline".to_string(), serde_json::Value::Bool(true));
        }

        // 按本采样窗口的 tick 差取前 3 个线程；累计 tick 只作附带字段。
        // 这样 top_threads 反映当前热点，而不是进程启动以来的历史累计。
        let top_threads: Vec<serde_json::Value> = match thread_samples {
            Some((_, samples)) => {
                let mut window: Vec<(u64, &ProcStat, u64)> = samples
                    .iter()
                    .map(|(tid, stat)| {
                        let current = stat.cpu_ticks();
                        let previous = previous_thread_ticks.get(tid).copied().unwrap_or(current);
                        (*tid, stat, current.saturating_sub(previous))
                    })
                    .collect();
                for (tid, stat) in &samples {
                    previous_thread_ticks.insert(*tid, stat.cpu_ticks());
                }
                window.sort_by_key(|entry| std::cmp::Reverse(entry.2));
                window
                    .iter()
                    .take(TOP_THREADS_LIMIT)
                    .map(|(tid, stat, delta)| {
                        serde_json::json!({
                            "tid": tid,
                            "name": stat.name,
                            "cpu_ticks": stat.cpu_ticks(),
                            "cpu_ticks_delta": delta,
                        })
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        fields.insert(
            "top_threads".to_string(),
            serde_json::Value::Array(top_threads),
        );

        writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            // sequence / session_id 由 writer_diagnostics::record_event 统一补全。
            sequence: 0,
            session_id: String::new(),
            level: writer_diagnostics::DiagnosticLevel::Info,
            origin: writer_diagnostics::DiagnosticOrigin::App,
            event: "process.resource".to_string(),
            target: "process.resource".to_string(),
            message: None,
            fields,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #665 评论 5643315523：未初始化时 effective_build_key/effective_package_type
    /// 回退到编译期常量，行为与原 BUILD_KEY/PACKAGE_TYPE 一致。
    #[test]
    fn test_effective_build_key_falls_back_to_compile_time_when_uninit() {
        // 未调用 init_build_identity() 时（OnceLock 未设置），应回退到编译期常量。
        // 注意：其他测试可能已调用 init_build_identity()，此时 EFFECTIVE_BUILD_KEY 已设置，
        // effective_build_key() 返回已设置的值。此测试只验证"未初始化"路径的逻辑正确性：
        // 即 effective_build_key() 返回的值要么是编译期 BUILD_KEY，要么是已初始化的有效值。
        let key = effective_build_key();
        assert!(!key.is_empty(), "effective_build_key must not be empty");
        let pkg = effective_package_type();
        assert!(!pkg.is_empty(), "effective_package_type must not be empty");
    }

    /// #665 评论 5643315523：APPIMAGE 不存在时，compute_effective_build_identity_with
    /// 返回编译期 PACKAGE_TYPE 和 BUILD_KEY。
    #[test]
    fn test_compute_effective_build_identity_no_appimage() {
        let (pkg_type, build_key) = compute_effective_build_identity_with(false);
        assert_eq!(pkg_type, PACKAGE_TYPE);
        assert_eq!(build_key, BUILD_KEY);
    }

    /// #665 评论 5643315523：APPIMAGE 存在时，compute_effective_build_identity_with
    /// 返回 "AppImage" 和重新组合的 build key（含 AppImage 而非编译期 packageType）。
    #[test]
    fn test_compute_effective_build_identity_with_appimage() {
        let (pkg_type, build_key) = compute_effective_build_identity_with(true);
        assert_eq!(pkg_type, "AppImage");

        if PACKAGE_TYPE == "AppImage" {
            // 编译期已是 AppImage，build key 应等于编译期 BUILD_KEY
            assert_eq!(build_key, BUILD_KEY);
        } else {
            // 编译期非 AppImage，build key 应重新组合，包含 "AppImage"
            assert_ne!(
                build_key, BUILD_KEY,
                "build key must differ when AppImage overrides compile-time package type"
            );
            assert!(
                build_key.contains("AppImage"),
                "build key must contain AppImage, got: {}",
                build_key
            );
            // 验证组合格式：{version}-{gitSha}-{AppImage}-{buildProfile}
            let expected = format!(
                "{}-{}-{}-{}",
                env!("CARGO_PKG_VERSION"),
                env!("GIT_COMMIT_SHA"),
                "AppImage",
                env!("BUILD_PROFILE")
            );
            assert_eq!(build_key, expected);
        }
    }

    /// #665 评论 5643315523：init_build_identity 幂等，多次调用安全。
    #[test]
    fn test_init_build_identity_is_idempotent() {
        // 多次调用不应 panic（OnceLock::set 对已设置的值返回 Err，但被 let _ = 忽略）
        init_build_identity();
        init_build_identity();
        // 验证 effective_* 函数仍能返回有效值
        let _ = effective_build_key();
        let _ = effective_package_type();
    }

    /// Issue #1061 第 6 项：正常 /proc/<pid>/stat 行解析。
    ///
    /// comm 之后的字段从 state 开始，utime/stime 是整体第 14/15 字段，
    /// 即 comm 之后 tokens[11]/tokens[12]。
    #[test]
    fn test_parse_proc_stat_normal_line() {
        let line = "4242 (sujian-linux-qt) S 1 4242 4242 0 -1 4194304 100 0 0 0 25 17 0 0 20 0 12 0 500 123456789 2345 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let Some(stat) = parse_proc_stat(line) else {
            panic!("normal proc stat line must parse");
        };
        assert_eq!(stat.name, "sujian-linux-qt");
        assert_eq!(stat.utime_ticks, 25);
        assert_eq!(stat.stime_ticks, 17);
        assert_eq!(stat.cpu_ticks(), 42);
    }

    /// Issue #1061 第 6 项：comm 含空格（及括号）时仍按首个 '(' 与最后一个 ')'
    /// 切分，不能整体 split_whitespace。
    #[test]
    fn test_parse_proc_stat_comm_with_spaces() {
        let line = "77 (sujian worker (io) 3) R 1 77 77 0 -1 4194304 10 0 0 0 8 2 0 0 20 0 1 0 100 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let Some(stat) = parse_proc_stat(line) else {
            panic!("proc stat line with spaces in comm must parse");
        };
        assert_eq!(stat.name, "sujian worker (io) 3");
        assert_eq!(stat.utime_ticks, 8);
        assert_eq!(stat.stime_ticks, 2);
        assert_eq!(stat.cpu_ticks(), 10);
    }
}
