//! Windows TSF/IME input surface adapter.
//!
//! Qt 的 windows QPA 插件把 TSF/IME 的 preedit、commit、replacement 和候选框
//! 锚点转换成标准 `QInputMethodEvent` / `QInputMethodQueryEvent`。这里只标注
//! Windows 平台名称；正文事务仍由共享 `platform_ime.rs` 写入 Rust EditorKernel。

use cpp::cpp;

cpp! {{
    #include <QGuiApplication>
    #include <QOperatingSystemVersion>
    #include <QString>
    #include <QOperatingSystemVersion>

    #if !defined(SUJIAN_PLATFORM_IME_ADAPTER_DEFINED)
    #define SUJIAN_PLATFORM_IME_ADAPTER_DEFINED 1
    class PlatformImeAdapter {
public:
        bool ime_composing;
        bool is_tsf;
        bool is_ms_ime;
        bool ime_detected;

        PlatformImeAdapter() : ime_composing(false), is_tsf(false), is_ms_ime(false), ime_detected(false) {}

        void detect_platform() {
            // Qt windows 插件通过 TSF 接入系统输入法；Windows 10/11 默认启用 TSF。
            is_tsf = true;
            ime_detected = true;
            is_ms_ime = QOperatingSystemVersion::current() >= QOperatingSystemVersion::Windows10;
        }

        const char* platform_name() const {
            if (is_tsf && is_ms_ime) return "windows_tsf";
            if (is_tsf) return "windows_ime";
            if (ime_detected) return "windows_ime";
            return "windows";
        }

        bool can_accept_plain_text_key() const { return !ime_composing; }
    };
    #endif
}}
