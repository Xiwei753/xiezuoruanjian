//! Windows input surface adapter.
//!
//! Windows is the only platform that owns pending-key deferred insertion.  The
//! first physical key may arrive as `QKeyEvent` before the matching
//! `QInputMethodEvent`; deferring by one queued turn prevents pinyin first-key
//! leakage.

use cpp::cpp;

cpp! {{
    #include <QString>

    extern "C" bool sujian_handle_key_and_text(void* rust_item, int key, int modifiers, const ushort* text, int text_len);

    #if defined(_WIN32) && !defined(SUJIAN_PLATFORM_IME_ADAPTER_DEFINED)
    #define SUJIAN_PLATFORM_IME_ADAPTER_DEFINED 1
    class PlatformImeAdapter {
    public:
        bool ime_composing;

    private:
        bool has_pending_key;
        QString pending_key_text;
        int pending_key_key;
        int pending_key_modifiers;

    public:
        PlatformImeAdapter()
            : ime_composing(false), has_pending_key(false),
              pending_key_key(0), pending_key_modifiers(0) {}

        void detect_platform() {}
        bool is_windows() const { return true; }
        bool is_linux() const { return false; }
        bool is_mac() const { return false; }
        const char* platform_name() const { return "windows"; }

        bool can_accept_plain_text_key() const {
            return !ime_composing && !has_pending_key;
        }

        void discard_pending_key() { has_pending_key = false; }

        void flush_pending_key(void* rust_item) {
            if (!has_pending_key) return;
            has_pending_key = false;
            if (pending_key_text.isEmpty()) return;
            sujian_handle_key_and_text(
                rust_item, pending_key_key, pending_key_modifiers,
                reinterpret_cast<const ushort*>(pending_key_text.utf16()),
                static_cast<int>(pending_key_text.size())
            );
        }

        void defer_key(int key, int modifiers, const QString& text) {
            pending_key_text = text;
            pending_key_key = key;
            pending_key_modifiers = modifiers;
            has_pending_key = true;
        }
    };
    #endif
}}
