//! Windows TSF/IME input surface adapter.
//!
//! Qt 的 windows QPA 插件把 TSF/IME 的 preedit、commit、replacement 和候选框
//! 锚点转换成标准 `QInputMethodEvent` / `QInputMethodQueryEvent`。共享
//! `qt_surface.rs` 负责把这些事件转换为 EditorKernel 事务；本适配器补上
//! Windows 当前键盘布局的 IME 识别与组合期间的 KeyPress 去重。

use cpp::cpp;

cpp! {{
    #include <QGuiApplication>
    #include <QKeyEvent>
    #include <QString>

    #ifdef Q_OS_WIN
    #include <windows.h>
    #include <imm.h>
    #endif

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
#ifdef Q_OS_WIN
            // Qt Windows 插件通过 TSF 接入输入法；是否正在使用系统 IME 由
            // 当前键盘布局决定，不能把 Windows 版本直接当成微软输入法。
            is_tsf = true;
            const HKL layout = GetKeyboardLayout(0);
            is_ms_ime = layout != nullptr && ImmIsIME(layout) != FALSE;
            ime_detected = is_tsf;
#else
            is_tsf = false;
            is_ms_ime = false;
            ime_detected = false;
#endif
        }

        const char* platform_name() const {
            if (is_ms_ime) return "windows_ms_ime";
            if (is_tsf) return "windows_tsf";
            return "windows";
        }

        bool can_accept_plain_text_key() const { return !ime_composing; }

        bool should_ignore_key_event(const QKeyEvent* event) const {
            // 组合期间 Windows IME 会继续投递 auto-repeat KeyPress；这些事件
            // 不是新的正文事务，交给 QInputMethodEvent 的 preedit/commit 处理。
            return ime_composing && event->isAutoRepeat();
        }
    };
    #endif
}}
