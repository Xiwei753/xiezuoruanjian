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
        bool ime_detected;

        PlatformImeAdapter() : ime_composing(false), is_wayland(false), is_fcitx5(false), is_ibus(false), ime_detected(false) {}

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

            ime_detected = is_fcitx5 || is_ibus;

            // Do NOT guess fcitx5=true when no IM is detected on Wayland.
            // If we cannot determine the IM, report "unknown" so the caller
            // can degrade gracefully instead of assuming a wrong platform.
        }

        const char* platform_name() const {
            if (is_wayland && is_fcitx5) return "wayland_fcitx5";
            if (is_wayland && is_ibus) return "wayland_ibus";
            if (is_fcitx5) return "x11_fcitx5";
            if (is_ibus) return "x11_ibus";
            if (is_wayland && !ime_detected) return "wayland_unknown";
            if (is_wayland) return "wayland";
            if (!ime_detected) return "linux_unknown";
            return "linux";
        }

        bool can_accept_plain_text_key() const { return !ime_composing; }

    };
    #endif
}}
