//! Linux/fcitx5/ibus input surface adapter.
//!
//! Qt on Linux delivers `QInputMethodEvent` with fcitx5/ibus semantics; plain text
//! keys are forwarded immediately and preedit/commit is handled by the Qt input
//! method event path in `qt_surface.rs`.

use cpp::cpp;

cpp! {{
    #include <QGuiApplication>
    #include <QString>

    #if !defined(SUJIAN_PLATFORM_IME_ADAPTER_DEFINED)
    #define SUJIAN_PLATFORM_IME_ADAPTER_DEFINED 1
    class PlatformImeAdapter {
    public:
        bool ime_composing;

        PlatformImeAdapter() : ime_composing(false) {}

        void detect_platform() {}
        const char* platform_name() const { return "linux"; }

        bool can_accept_plain_text_key() const { return !ime_composing; }

    };
    #endif
}}
