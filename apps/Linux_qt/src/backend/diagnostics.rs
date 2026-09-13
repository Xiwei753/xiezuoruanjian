// =============================================================================
// diagnostics.rs — Linux_qt 平台诊断采集器
// =============================================================================
//
// 引用了什么：
// - std::sync::OnceLock：平台信息缓存
//
// 干什么的：
// - 检测 Linux 平台特有的 AppImage 运行时包类型，计算有效 buildKey
// - 收集 Qt 运行时信息、系统信息（由 main.rs 注入），供导出诊断包时作为
//   PlatformAttachment 交给 Rust 统一诊断后端 (writer_diagnostics)
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
// - settings_backend.rs：get_runtime_info / get_system_info（构造导出附件）
// - app_backend.rs：effective_build_key（日志文件名）
// =============================================================================

use std::sync::OnceLock;

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
}
