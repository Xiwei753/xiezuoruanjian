//! Linux/fcitx5/ibus input surface adapter.
//!
//! Linux must not carry Windows pending-key deferred insertion state.  Qt on
//! Linux delivers `QInputMethodEvent` with fcitx5/ibus semantics; plain text
//! keys are forwarded immediately and preedit/commit is handled by the Qt input
//! method event path in `qt_surface.rs`.

use cpp::cpp;

cpp! {{
    #include <QGuiApplication>
    #include <QString>

    #if !defined(_WIN32) && !defined(SUJIAN_PLATFORM_IME_ADAPTER_DEFINED)
    #define SUJIAN_PLATFORM_IME_ADAPTER_DEFINED 1
    class PlatformImeAdapter {
    public:
        bool ime_composing;

        PlatformImeAdapter() : ime_composing(false) {}

        void detect_platform() {}
        bool is_windows() const { return false; }
        bool is_linux() const { return true; }
        bool is_mac() const { return false; }
        const char* platform_name() const { return "linux"; }

        bool can_accept_plain_text_key() const { return !ime_composing; }

        void discard_pending_key() {}
        void flush_pending_key(void*) {}
        void defer_key(int, int, const QString&) {}
    };
    #endif
}}
