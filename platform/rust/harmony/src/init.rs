//! HarmonyOS 平台初始化构造与诊断后端接入。
//!
//! Harmony 平台通过 NAPI 桥接层把目录信息传进来，构造 `PlatformInit`
//! 并启动统一日志后端。

use std::io::Read;
use std::path::{Path, PathBuf};

use writer_platform_api::{PlatformInit, PlatformKind};

/// 检查文件是否真的包含 PEM 证书，而不是把任意可读文件当作 CA。
fn has_ca_certificate(path: &Path) -> bool {
    const MARKER: &[u8] = b"-----BEGIN CERTIFICATE-----";
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buffer = [0u8; 64];
    match file.read(&mut buffer) {
        Ok(size) => buffer[..size]
            .windows(MARKER.len())
            .any(|window| window == MARKER),
        Err(_) => false,
    }
}

/// 目录必须实际包含可供 OpenSSL 使用的 CA 文件，仅有读权限不足。
fn directory_has_ca_files(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries
        .take(256)
        .flatten()
        .any(|entry| entry.path().is_file() && has_ca_certificate(&entry.path()))
}

/// HarmonyOS 系统 CA 的候选位置。
///
/// 华为《网络连接安全配置》要求第三方库使用系统预置 CA 目录
/// `/etc/security/certificates`。该目录优先于其他系统的兼容路径；兼容路径
/// 只有在确实包含 PEM 证书时才会被采用。禁止在没有可信根时关闭 TLS 校验，
/// 未发现可用 CA 时由 Core 返回 `tls_ca_unavailable`。
fn resolve_tls_ca() -> (Option<PathBuf>, Option<PathBuf>) {
    let bundle_candidates = [
        std::env::var_os("SSL_CERT_FILE").map(PathBuf::from),
        Some(PathBuf::from("/etc/ssl/certs/cacert.pem")),
        Some(PathBuf::from("/system/etc/ssl/certs/cacert.pem")),
        Some(PathBuf::from("/system/etc/security/cacert.pem")),
        Some(PathBuf::from(
            "/system/etc/security/certificates/cacert.pem",
        )),
        Some(PathBuf::from("/etc/ssl/certs/ca-certificates.crt")),
    ];
    let dir_candidates = [
        Some(PathBuf::from("/etc/security/certificates")),
        Some(PathBuf::from("/system/etc/security/certificates")),
        std::env::var_os("SSL_CERT_DIR").map(PathBuf::from),
        Some(PathBuf::from("/etc/ssl/certs")),
    ];
    (
        bundle_candidates
            .into_iter()
            .flatten()
            .find(|path| path.is_file() && has_ca_certificate(path)),
        dir_candidates
            .into_iter()
            .flatten()
            .find(|path| path.is_dir() && directory_has_ca_files(path)),
    )
}

/// 从 Harmony 目录信息构造平台初始化结构。
pub fn create_platform_init(
    files_dir: PathBuf,
    cache_dir: PathBuf,
    device_id: String,
    app_version: String,
    locale: String,
    timezone: String,
) -> PlatformInit {
    let log_dir = cache_dir.join("log");
    let (tls_ca_bundle, tls_ca_dir) = resolve_tls_ca();
    PlatformInit {
        platform: PlatformKind::Harmony,
        app_data_dir: files_dir,
        cache_dir,
        log_dir,
        no_backup_dir: None,
        device_id,
        app_version,
        locale,
        timezone,
        tls_ca_bundle,
        tls_ca_dir,
        tls_ca_required: true,
    }
}

/// 把完整 `PlatformInit` 交给 `writer_diagnostics::init`，启动统一日志后端。
///
/// 幂等：重复调用无副作用。
pub fn init_diagnostics(
    init: &PlatformInit,
    build_key: String,
    session_id: String,
    enabled: bool,
    verbose: bool,
) {
    let config = writer_diagnostics::DiagnosticsConfig {
        log_dir: init.log_dir.clone(),
        platform_name: init.platform.to_string(),
        device_id: init.device_id.clone(),
        app_version: init.app_version.clone(),
        build_key,
        locale: init.locale.clone(),
        timezone: init.timezone.clone(),
        session_id,
        enabled,
        verbose,
    };
    writer_diagnostics::init(config);
}

// ── C-ABI diagnostics init 入口 ──

use std::ffi::CStr;
use std::os::raw::c_char;

/// C-ABI diagnostics init 入口 — Issue #670 评论 5652119660 修复 2a / #760 诊断导出 / #911 启动设置读取。
///
/// 供 NAPI 桥接层（`napi_init.cpp` 的 `NativeInitDiagnostics`）调用，
/// 把目录/设备/版本/locale/timezone 等信息从 ArkTS 传进 Rust，
/// 从 `app_data_root` 下的 `settings.local.json` 读取**真实**诊断开关，
/// 构造 `DiagnosticsConfig` 并启动统一日志后端。
///
/// #911：此前 `enabled/verbose` 写死 `true/true`，启动日志是否开启与用户配置不一致，
/// 只有用户打开设置页才会补关。这里在构造 config 之前先用仓库现成的
/// `writer_core::settings::load_local_settings` 读同一份 Core LocalSettings，
/// 不新建设置副本、不在 ArkTS 重写 JSON 解析器。
///
/// ## 参数
///
/// 所有 `*const c_char` 参数均为 NUL-terminated UTF-8 C 字符串。任一为 null 时
/// 返回 `-1`（无效参数），不构造 config、不调用 `writer_diagnostics::init`。
/// `app_data_root` 是 WriterCore 数据根，`settings.local.json` 位于其下。
///
/// ## 返回码
///
/// - `0`  成功
/// - `-1` 任一参数为 null
/// - `-2` 任一参数含无效 UTF-8
/// - `-3` 本地设置读取/解析失败（不假称已应用用户配置）
///
/// ## Safety
///
/// 调用方必须保证非 null 参数指向有效的 NUL-terminated UTF-8 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn writer_core_init_diagnostics(
    log_dir: *const c_char,
    app_data_root: *const c_char,
    device_id: *const c_char,
    app_version: *const c_char,
    build_key: *const c_char,
    locale: *const c_char,
    timezone: *const c_char,
) -> i32 {
    // SAFETY: 各参数 null 检查后再 CStr::from_ptr；调用方保证非 null 指针指向有效 NUL-terminated UTF-8。
    let log_dir_str = match c_str_to_rust(log_dir) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let app_data_root_str = match c_str_to_rust(app_data_root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let device_id_str = match c_str_to_rust(device_id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let app_version_str = match c_str_to_rust(app_version) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let build_key_str = match c_str_to_rust(build_key) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let locale_str = match c_str_to_rust(locale) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let timezone_str = match c_str_to_rust(timezone) {
        Ok(s) => s,
        Err(e) => return e,
    };

    // session_id 在 Rust 里生成 — 每次启动唯一。
    let session_id = uuid::Uuid::new_v4().to_string();

    // 启动即读 Core 真实本地设置：`load_local_settings` 已处理文件不存在时的 Core
    // 默认值与旧设置迁移。读取/解析失败返回 -3，不继续用写死的 true/true 冒充用户配置。
    let settings = match writer_core::settings::load_local_settings(std::path::Path::new(
        &app_data_root_str,
    )) {
        Ok(s) => s,
        Err(e) => {
            log::error!("writer_core_init_diagnostics: load_local_settings failed: {e}");
            return -3;
        }
    };

    let config = writer_diagnostics::DiagnosticsConfig {
        log_dir: std::path::PathBuf::from(log_dir_str),
        platform_name: "harmony".to_string(),
        device_id: device_id_str,
        app_version: app_version_str,
        build_key: build_key_str,
        locale: locale_str,
        timezone: timezone_str,
        session_id,
        enabled: settings.diagnostics_enabled,
        verbose: settings.diagnostics_verbose,
    };
    writer_diagnostics::init(config);
    0
}

/// 将 C string 转换为 Rust `String`（init 模块私有辅助）。
///
/// ## 错误码
///
/// - `-1`：空指针
/// - `-2`：无效 UTF-8
fn c_str_to_rust(s: *const c_char) -> Result<String, i32> {
    if s.is_null() {
        return Err(-1);
    }
    // SAFETY: s is null-checked above; the C ABI caller guarantees a valid NUL-terminated UTF-8 string.
    match unsafe { CStr::from_ptr(s) }.to_str() {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Err(-2),
    }
}

// ── C-ABI Harmony 专用初始化入口 ──

/// Harmony 专用 C-ABI 初始化入口。
///
/// 由 NAPI 桥接层调用，把目录/设备/版本/locale/timezone 等信息从 ArkTS 传进 Rust，
/// 构造 Harmony `PlatformServices`（含 `SyncTransportFactory`），再通过
/// `writer_core_init_with_services` 初始化同一份全局 `APP_SERVICE`。
///
/// 与 `writer_core_init` 的区别：本入口注入 `PlatformServices`（含同步传输工厂），
/// 让 Harmony 平台具备 GitHub 同步能力。`writer_core_init` 仍保留供不带同步的
/// 简单初始化场景使用。
///
/// ## 参数
///
/// 所有 `*const c_char` 参数均为 NUL-terminated UTF-8 C 字符串。`app_data_root`
/// 为 null 或空串时返回 `-5`（非法路径），不做隐式 cwd 兜底。
/// `files_dir` 为 null / 空串时回退到 `app_data_root`（两者本应指向同一 WriterCore 数据根）；
/// `cache_dir` 为 null / 空串时回退到 `app_data_root`。`device_id` / `app_version` /
/// `locale` / `timezone` 为 null 时使用空字符串。
/// `is_connected` 和 `is_metered` 来自 ArkTS 网络状态查询，是三态：
/// - `1` = 在线 / 计费
/// - `0` = 离线 / 非计费
/// - `-1` = 未知（查询失败）
///
/// 连通性为未知时不注入伪造的在线状态，`network_state` 记为 `None`。
///
/// ## 返回码
///
/// - `0`  成功
/// - `-1` `app_data_root` 为 null
/// - `-2` 任一参数含无效 UTF-8
/// - `-4` bootstrap 失败
/// - `-5` `app_data_root` 为空串（非法路径）
/// - `-6` 安全存储目录创建等本地初始化失败（不回落明文凭据路径；
///   HUKS 可用性不在启动时探测，安全能力故障在凭证操作时明确返回）
///
/// ## Safety
///
/// 调用方必须保证非 null 参数指向有效的 NUL-terminated UTF-8 C 字符串。
#[no_mangle]
pub unsafe extern "C" fn writer_core_init_harmony(
    app_data_root: *const c_char,
    files_dir: *const c_char,
    cache_dir: *const c_char,
    device_id: *const c_char,
    app_version: *const c_char,
    locale: *const c_char,
    timezone: *const c_char,
    is_connected: i32,
    is_metered: i32,
) -> i32 {
    let app_data_root_str = match c_str_to_rust(app_data_root) {
        Ok(s) => s,
        Err(e) => return e,
    };
    // 明确拒绝空路径：不允许隐式以进程 cwd 作为配置目录。
    if app_data_root_str.trim().is_empty() {
        log::error!("writer_core_init_harmony: app_data_root is empty, refusing to init");
        return -5;
    }
    // files_dir 默认与 app_data_root 相同，二者应是同一个 WriterCore 数据根，
    // 这样 PlatformInit.app_data_dir 与 Core 的 app_data_root 配置契约一致。
    let files_dir_str = non_empty_or(files_dir, &app_data_root_str);
    // cache_dir 必须是真实缓存目录；缺失时回退到数据根，但记录警告。
    let cache_dir_str = match c_str_to_rust(cache_dir) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => {
            log::warn!(
                "writer_core_init_harmony: cache_dir missing, falling back to app_data_root"
            );
            app_data_root_str.clone()
        }
    };
    let device_id_str = c_str_to_rust(device_id).unwrap_or_default();
    let app_version_str = c_str_to_rust(app_version).unwrap_or_default();
    let locale_str = c_str_to_rust(locale).unwrap_or_default();
    let timezone_str = c_str_to_rust(timezone).unwrap_or_default();

    let platform_init = create_platform_init(
        PathBuf::from(files_dir_str),
        PathBuf::from(cache_dir_str),
        device_id_str,
        app_version_str,
        locale_str,
        timezone_str,
    );

    // 三态网络状态：-1 = 未知，0 = 否，1 = 是。
    let connected = tri_state(is_connected);
    let metered = tri_state(is_metered);

    // 安全存储实例只在构造时建立目录；HUKS 可用性不在这里探测（Issue #924 复核），
    // 因此这里的失败只可能是目录创建等本地错误，以 -6 结束，不悄悄回退明文文件路径。
    let services =
        match super::services::create_platform_services(platform_init, connected, metered) {
            Ok(services) => services,
            Err(error) => {
                log::error!("writer_core_init_harmony: secure storage setup failed: {error}");
                return -6;
            }
        };

    // SAFETY: app_data_root_str 是有效的 NUL-terminated UTF-8 C string 的 Rust 拷贝。
    let path_c = std::ffi::CString::new(app_data_root_str).unwrap_or_default();
    // SAFETY: path_c 是有效的 NUL-terminated UTF-8 C string，services 是 Rust 端构造的合法 PlatformServices。
    unsafe { writer_core::ffi::writer_core_init_with_services(path_c.as_ptr(), services) }
}

/// 把 C ABI 的三态整数转换为 `Option<bool>`：`-1`（或任意负数）= 未知。
fn tri_state(value: i32) -> Option<bool> {
    match value {
        1 => Some(true),
        0 => Some(false),
        _ => None,
    }
}

/// 非空则用提供的路径，否则回退到默认值。
fn non_empty_or(ptr: *const c_char, default: &str) -> String {
    match c_str_to_rust(ptr) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => default.to_string(),
    }
}
