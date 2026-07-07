//! Linux/fcitx5/ibus input surface adapter.
//!
//! Qt on Linux delivers `QInputMethodEvent` with fcitx5/ibus semantics; plain text
//! keys are forwarded immediately and preedit/commit is handled by the Qt input
//! method event path in `qt_surface.rs`.

use cpp::cpp;

cpp! {{
    #include <QGuiApplication>
    #include <QString>
    #include <QProcessEnvironment>

    #if !defined(SUJIAN_PLATFORM_IME_ADAPTER_DEFINED)
    #define SUJIAN_PLATFORM_IME_ADAPTER_DEFINED 1
    class PlatformImeAdapter {
    public:
        bool ime_composing;
        bool is_wayland;
        bool is_fcitx5;
        bool is_ibus;

        PlatformImeAdapter() : ime_composing(false), is_wayland(false), is_fcitx5(false), is_ibus(false) {}

        void detect_platform() {
            QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
            QString xdgSessionType = env.value("XDG_SESSION_TYPE").toLower();
            QString qtImModule = env.value("QT_IM_MODULE").toLower();
            QString qtImModules = env.value("QT_IM_MODULES").toLower();

            is_wayland = (xdgSessionType == "wayland");

            if (qtImModule.contains("fcitx") || qtImModules.contains("fcitx")) {
                is_fcitx5 = true;
            }
            if (qtImModule.contains("ibus") || qtImModules.contains("ibus")) {
                is_ibus = true;
            }

            if (!is_fcitx5 && !is_ibus && is_wayland) {
                is_fcitx5 = true;
            }
        }

        const char* platform_name() const {
            if (is_wayland && is_fcitx5) return "wayland_fcitx5";
            if (is_wayland && is_ibus) return "wayland_ibus";
            if (is_fcitx5) return "x11_fcitx5";
            if (is_ibus) return "x11_ibus";
            if (is_wayland) return "wayland";
            return "linux";
        }

        bool can_accept_plain_text_key() const { return !ime_composing; }

    };
    #endif
}}
