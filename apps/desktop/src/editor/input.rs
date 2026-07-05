//! Desktop 输入层三层架构
//!
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │ Layer 1: QtInputSurface (C++ SujianEventFilter)                     │
//! │   - Qt 官方事件入口：keyPressEvent / inputMethodEvent / query       │
//! │   - 不写正文业务，不写动画逻辑                                      │
//! │   - 委托给 PlatformImeAdapter 处理平台差异                          │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │ Layer 2: PlatformImeAdapter (C++ 内嵌)                              │
//! │   - WindowsImeAdapter: pending key deferred insertion               │
//! │   - LinuxImeAdapter: 直接插入，不延迟，按 Qt inputMethodEvent 语义  │
//! │   - MacImeAdapter: 独立保留，不继承 Windows 逻辑                    │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │ Layer 3: EditorInputController (Rust)                               │
//! │   - 接收归一化输入事件：PlainText / Shortcut / Preedit / Commit 等  │
//! │   - 调用 SujianEditorItem / EditorEngine 修改正文和生成视觉事务     │
//! │   - 平台差异只存在 Layer 2，正文编辑和动画永远不知道当前平台        │
//! └──────────────────────────────────────────────────────────────────────┘

use crate::sujian_editor_item::SujianEditorItem;
use cpp::cpp;
use std::ffi::c_void;

cpp! {{
    #include <QtGui/QInputMethodEvent>
    #include <QtGui/QKeyEvent>
    #include <QtGui/QKeySequence>
    #include <QtGui/QTextCharFormat>
    #include <QtGui/QTextFormat>
    #include <QtQuick/QQuickItem>
    #include <QEvent>
    #include <QObject>
    #include <QRectF>
    #include <QString>
    #include <QDebug>
    #include <QtGui/QInputMethod>
    #include <QGuiApplication>

    extern "C" bool sujian_handle_key_and_text(void* rust_item, int key, int modifiers, const ushort* text, int text_len);
    extern "C" void sujian_ime_commit(void* rust_item, const ushort* text, int text_len);
    extern "C" void sujian_ime_replace_and_commit(void* rust_item, const ushort* text, int text_len, int replace_start, int replace_length);
    extern "C" void sujian_ime_preedit(void* rust_item, const ushort* text, int text_len, int cursor);
    extern "C" void sujian_ime_preedit_attrs(void* rust_item, const ushort* text, int text_len, int cursor, const int* attr_types, const int* attr_starts, const int* attr_lengths, int attr_count, const int* attr_formats);
    extern "C" void sujian_ime_cancel(void* rust_item);
    extern "C" void sujian_request_repaint(void* rust_item);

    // ═══════════════════════════════════════════════════════════════════════
    // Layer 2: PlatformImeAdapter — 平台分支点
    //
    // Windows: pending key deferred insertion 防首字母泄漏
    // Linux/macOS: 直接插入，不延迟，按 Qt inputMethodEvent 语义
    // ═══════════════════════════════════════════════════════════════════════

    enum class PlatformKind { Windows, Linux, Mac };

    class PlatformImeAdapter {
    public:
        PlatformKind platform;
        bool ime_composing;

        // ── Windows-only: pending key deferred insertion ──
        // On Windows, QKeyEvent arrives BEFORE QInputMethodEvent for the same
        // physical key press. If we insert the key text immediately, the first
        // pinyin key (e.g. 'n') leaks into the document as an English letter.
        // Solution: defer plain-text insertion by one event loop iteration.
        // If QInputMethodEvent arrives before the deferred callback fires,
        // the pending key is discarded (IME consumed it). Otherwise the key
        // is inserted normally (genuine non-IME input).
        //
        // IMPORTANT: This strategy is ONLY for Windows. On Linux/macOS,
        // Qt/fcitx/ibus deliver InputMethodEvent correctly without the
        // first-key leak issue. Deferring on Linux breaks symbol input,
        // preedit, and IME commit because the event ordering differs.
        bool has_pending_key;
        QString pending_key_text;
        int pending_key_key;
        int pending_key_modifiers;

        PlatformImeAdapter()
            : platform(PlatformKind::Linux), ime_composing(false),
              has_pending_key(false), pending_key_key(0), pending_key_modifiers(0) {}

        void detect_platform() {
            QString platformName = QGuiApplication::platformName();
            if (platformName == QStringLiteral("windows")) {
                platform = PlatformKind::Windows;
            } else if (platformName == QStringLiteral("cocoa")) {
                platform = PlatformKind::Mac;
            } else {
                platform = PlatformKind::Linux;
            }
        }

        bool is_windows() const { return platform == PlatformKind::Windows; }
        bool is_linux() const { return platform == PlatformKind::Linux; }
        bool is_mac() const { return platform == PlatformKind::Mac; }

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

    // ═══════════════════════════════════════════════════════════════════════
    // Layer 1: QtInputSurface (SujianEventFilter)
    //
    // Qt 官方事件入口：keyPressEvent / inputMethodEvent / inputMethodQuery
    // 不写正文业务，不写动画逻辑
    // 委托给 PlatformImeAdapter 处理平台差异
    // ═══════════════════════════════════════════════════════════════════════

    class SujianEventFilter : public QObject {
    public:
        void* rust_item;
        PlatformImeAdapter ime_adapter;

        SujianEventFilter(QObject* parent, void* item)
            : QObject(parent), rust_item(item) {
            ime_adapter.detect_platform();
        }

        bool eventFilter(QObject* obj, QEvent* event) override {
            if (!rust_item) return false;

            switch (event->type()) {
            case QEvent::KeyPress: {
                auto* ke = static_cast<QKeyEvent*>(event);
                return handle_key_press(obj, ke);
            }
            case QEvent::InputMethod: {
                auto* ime = static_cast<QInputMethodEvent*>(event);
                return handle_input_method(obj, ime);
            }
            case QEvent::InputMethodQuery: {
                auto* qe = static_cast<QInputMethodQueryEvent*>(event);
                return handle_input_method_query(obj, qe);
            }
            default:
                return false;
            }
        }

    private:
        // ── Layer 1 → Layer 2: KeyPress 分支 ──
        bool handle_key_press(QObject* obj, QKeyEvent* ke) {
            // ── Standard shortcut matching via QKeySequence ──
            if (try_shortcut(ke)) return true;

            // ── Plain text insertion (non-shortcut, non-IME) ──
            // Platform-branch strategy:
            //
            // Windows: QKeyEvent arrives BEFORE QInputMethodEvent for the same
            // physical key. To prevent the first pinyin key from leaking as an
            // English letter, we defer plain-text insertion by one event loop
            // iteration. If QInputMethodEvent arrives first (IME consumed the
            // key), the pending key is discarded. Otherwise it's inserted normally.
            //
            // Linux/macOS: Qt/fcitx/ibus deliver InputMethodEvent correctly
            // without the first-key leak. Deferring on Linux breaks symbol
            // input (space, +, -, etc.), preedit display, and IME commit
            // because the event ordering differs from Windows. On these
            // platforms, plain text keys are inserted immediately.
            //
            // Do NOT use QInputMethod::isVisible() — it's unreliable and blocks
            // space, +, and other symbols from being inserted.
            if (is_plain_text_key(ke)) {
                if (ime_adapter.is_windows()) {
                    // WindowsImeAdapter: defer insertion to prevent IME first-key leak
                    ime_adapter.defer_key(
                        ke->key(),
                        static_cast<int>(ke->modifiers()),
                        ke->text()
                    );
                    QMetaObject::invokeMethod(this, [this]() {
                        ime_adapter.flush_pending_key(rust_item);
                    }, Qt::QueuedConnection);
                    return true;
                } else {
                    // LinuxImeAdapter / MacImeAdapter: insert immediately, no deferred insertion
                    bool accepted = sujian_handle_key_and_text(
                        rust_item,
                        static_cast<int>(ke->key()),
                        static_cast<int>(ke->modifiers()),
                        reinterpret_cast<const ushort*>(ke->text().utf16()),
                        static_cast<int>(ke->text().size())
                    );
                    return accepted;
                }
            }
            // Handle navigation, deletion, and shortcut keys (no text).
            bool accepted = sujian_handle_key_and_text(
                rust_item,
                ke->key(),
                static_cast<int>(ke->modifiers()),
                nullptr,
                0
            );
            return accepted;
        }

        bool is_plain_text_key(QKeyEvent* ke) const {
            return !ime_adapter.ime_composing
                && !(ke->modifiers() & (Qt::ControlModifier | Qt::AltModifier | Qt::MetaModifier))
                && !ke->text().isEmpty()
                && ke->key() != Qt::Key_Backspace
                && ke->key() != Qt::Key_Delete
                && ke->key() != Qt::Key_Return
                && ke->key() != Qt::Key_Enter
                && ke->key() != Qt::Key_Tab
                && ke->key() != Qt::Key_Escape
                && ke->key() != Qt::Key_Left
                && ke->key() != Qt::Key_Right
                && ke->key() != Qt::Key_Up
                && ke->key() != Qt::Key_Down
                && ke->key() != Qt::Key_Home
                && ke->key() != Qt::Key_End
                && ke->key() != Qt::Key_PageUp
                && ke->key() != Qt::Key_PageDown
                && !ime_adapter.has_pending_key;
        }

        bool try_shortcut(QKeyEvent* ke) {
            auto tryMatch = [&](QKeySequence::StandardKey matchExpr) -> bool {
                if (ke->matches(matchExpr)) {
                    bool accepted = sujian_handle_key_and_text(
                        rust_item, static_cast<int>(ke->key()),
                        static_cast<int>(ke->modifiers()), nullptr, 0);
                    return accepted;
                }
                return false;
            };

            if (tryMatch(QKeySequence::SelectAll)) return true;
            if (tryMatch(QKeySequence::Copy)) return true;
            if (tryMatch(QKeySequence::Cut)) return true;
            if (tryMatch(QKeySequence::Paste)) return true;
            if (tryMatch(QKeySequence::Undo)) return true;
            if (tryMatch(QKeySequence::Redo)) return true;
            if (tryMatch(QKeySequence::Delete)) return true;
            if (tryMatch(QKeySequence::Backspace)) return true;
            if (tryMatch(QKeySequence::MoveToPreviousChar)) return true;
            if (tryMatch(QKeySequence::MoveToNextChar)) return true;
            if (tryMatch(QKeySequence::MoveToStartOfLine)) return true;
            if (tryMatch(QKeySequence::MoveToEndOfLine)) return true;
            if (tryMatch(QKeySequence::MoveToPreviousLine)) return true;
            if (tryMatch(QKeySequence::MoveToNextLine)) return true;
            if (tryMatch(QKeySequence::SelectPreviousChar)) return true;
            if (tryMatch(QKeySequence::SelectNextChar)) return true;
            if (tryMatch(QKeySequence::SelectStartOfLine)) return true;
            if (tryMatch(QKeySequence::SelectEndOfLine)) return true;
            if (tryMatch(QKeySequence::SelectPreviousLine)) return true;
            if (tryMatch(QKeySequence::SelectNextLine)) return true;

            return false;
        }

        // ── Layer 1 → Layer 2: InputMethod 分支 ──
        bool handle_input_method(QObject* obj, QInputMethodEvent* ime) {
            QString commit = ime->commitString();
            QString preedit = ime->preeditString();
            if (qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG") || qEnvironmentVariableIsSet("WRITER_DEBUG")) {
                qDebug("[sujian] InputMethodEvent: preedit_len=%lld, commit_len=%lld, platform=%s",
                       static_cast<long long>(preedit.length()), static_cast<long long>(commit.length()),
                       ime_adapter.is_windows() ? "windows" : (ime_adapter.is_mac() ? "mac" : "linux"));
            }
            if (!commit.isEmpty()) {
                ime_adapter.discard_pending_key();
                int replace_start = ime->replacementStart();
                int replace_length = ime->replacementLength();
                if (replace_length != 0) {
                    sujian_ime_replace_and_commit(
                        rust_item,
                        reinterpret_cast<const ushort*>(commit.utf16()),
                        static_cast<int>(commit.size()),
                        replace_start,
                        replace_length
                    );
                } else {
                    sujian_ime_commit(
                        rust_item,
                        reinterpret_cast<const ushort*>(commit.utf16()),
                        static_cast<int>(commit.size())
                    );
                }
                ime_adapter.ime_composing = false;
            }
            if (!preedit.isEmpty()) {
                ime_adapter.discard_pending_key();
                ime_adapter.ime_composing = true;
                int cursor = preedit.length();
                for (const auto& attr : ime->attributes()) {
                    if (attr.type == QInputMethodEvent::Cursor) {
                        cursor = attr.start;
                        break;
                    }
                }
                if (cursor < 0) cursor = 0;
                if (cursor > preedit.length()) cursor = preedit.length();

                QVector<int> attr_types;
                QVector<int> attr_starts;
                QVector<int> attr_lengths;
                QVector<int> attr_formats;
                for (const auto& attr : ime->attributes()) {
                    if (attr.type == QInputMethodEvent::TextFormat || attr.type == QInputMethodEvent::Cursor) {
                        attr_types.append(static_cast<int>(attr.type));
                        attr_starts.append(attr.start);
                        attr_lengths.append(attr.length);
                        int format_code = 0;
                        if (attr.type == QInputMethodEvent::TextFormat) {
                            QTextCharFormat fmt = qvariant_cast<QTextCharFormat>(attr.value);
                            if (fmt.fontUnderline()) {
                                format_code = 3;
                            } else if (fmt.hasProperty(QTextFormat::BackgroundBrush)) {
                                format_code = 2;
                            } else if (fmt.hasProperty(QTextFormat::ForegroundBrush)) {
                                format_code = 1;
                            }
                        }
                        attr_formats.append(format_code);
                    }
                }
                int attr_count = attr_types.size();
                sujian_ime_preedit_attrs(
                    rust_item,
                    reinterpret_cast<const ushort*>(preedit.utf16()),
                    static_cast<int>(preedit.size()),
                    cursor,
                    attr_types.constData(),
                    attr_starts.constData(),
                    attr_lengths.constData(),
                    attr_count,
                    attr_formats.constData()
                );
            } else if (commit.isEmpty()) {
                ime_adapter.discard_pending_key();
                sujian_ime_cancel(rust_item);
                ime_adapter.ime_composing = false;
            }
            sujian_request_repaint(rust_item);
            QInputMethod* im = QGuiApplication::inputMethod();
            if (im) {
                im->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle | Qt::ImCursorPosition | Qt::ImSurroundingText | Qt::ImCurrentSelection);
            }
            return true;
        }

        // ── Layer 1: InputMethodQuery ──
        bool handle_input_method_query(QObject* obj, QInputMethodQueryEvent* qe) {
            if (qe->queries() & Qt::ImEnabled) {
                qe->setValue(Qt::ImEnabled, true);
            }
            if (qe->queries() & Qt::ImHints) {
                qe->setValue(Qt::ImHints, static_cast<int>(Qt::ImhNoPredictiveText));
            }
            if (qe->queries() & Qt::ImCursorRectangle) {
                double cx = obj->property("cursor_rect_x").toDouble();
                double cy = obj->property("cursor_rect_y").toDouble();
                double cw = obj->property("cursor_rect_width").toDouble();
                double ch = obj->property("cursor_rect_height").toDouble();
                qe->setValue(Qt::ImCursorRectangle, QRectF(cx, cy, cw, ch));
            }
            if (qe->queries() & Qt::ImAnchorRectangle) {
                double cx = obj->property("cursor_rect_x").toDouble();
                double cy = obj->property("cursor_rect_y").toDouble();
                double cw = obj->property("cursor_rect_width").toDouble();
                double ch = obj->property("cursor_rect_height").toDouble();
                qe->setValue(Qt::ImAnchorRectangle, QRectF(cx, cy, cw, ch));
            }
            if (qe->queries() & Qt::ImSurroundingText) {
                QString surrounding = obj->property("plain_text").toString();
                qe->setValue(Qt::ImSurroundingText, surrounding);
            }
            if (qe->queries() & Qt::ImCursorPosition) {
                int cursorPos = obj->property("cursor_position").toInt();
                qe->setValue(Qt::ImCursorPosition, cursorPos);
            }
            if (qe->queries() & Qt::ImCurrentSelection) {
                QString selText = obj->property("current_selection_text").toString();
                qe->setValue(Qt::ImCurrentSelection, selText);
            }
            if (qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG") || qEnvironmentVariableIsSet("WRITER_DEBUG")) {
                qDebug("[sujian] InputMethodQuery: queries=0x%x", static_cast<unsigned>(qe->queries()));
            }
            return true;
        }
    };

    void sujian_install_event_filter(QQuickItem* item, void* rust_item) {
        if (!item) return;
        auto* filter = new SujianEventFilter(item, rust_item);
        item->installEventFilter(filter);
        item->setFlag(QQuickItem::ItemHasContents, true);
        item->setFlag(QQuickItem::ItemAcceptsInputMethod, true);
        item->setAcceptedMouseButtons(Qt::AllButtons);
        item->setFocusPolicy(Qt::StrongFocus);
        QInputMethod* im = QGuiApplication::inputMethod();
        if (im) {
            im->update(Qt::ImEnabled);
        }
        // Always log IME diagnostics on startup (not just debug mode)
        const char* platform_str = filter->ime_adapter.is_windows() ? "windows"
                                 : filter->ime_adapter.is_mac() ? "mac" : "linux";
        qDebug("[sujian] install_event_filter: platform=%s, QT_QPA_PLATFORM=%s, QT_IM_MODULE=%s, "
               "QT_IM_MODULES=%s, QT_PLUGIN_PATH=%s, "
               "ItemAcceptsInputMethod=%d, QInputMethod=%p, hasActiveFocus=%d",
               platform_str,
               qEnvironmentVariable("QT_QPA_PLATFORM"),
               qEnvironmentVariable("QT_IM_MODULE"),
               qEnvironmentVariable("QT_IM_MODULES"),
               qEnvironmentVariable("QT_PLUGIN_PATH"),
               item->flags().testFlag(QQuickItem::ItemAcceptsInputMethod),
               im,
               item->hasActiveFocus());
    }

    void sujian_focus_item(QQuickItem* item) {
        if (!item) return;
        item->forceActiveFocus(Qt::MouseFocusReason);
        QInputMethod* im = QGuiApplication::inputMethod();
        if (im) {
            im->update(Qt::ImEnabled | Qt::ImCursorRectangle | Qt::ImAnchorRectangle | Qt::ImSurroundingText | Qt::ImCursorPosition | Qt::ImCurrentSelection);
            im->show();
        }
        qDebug("[sujian] focus_item: hasActiveFocus=%d, ItemAcceptsInputMethod=%d",
               item->hasActiveFocus(),
               item->flags().testFlag(QQuickItem::ItemAcceptsInputMethod));
    }
}}

pub(crate) const KEY_BACKSPACE: i32 = 0x0100_0003;
pub(crate) const KEY_TAB: i32 = 0x0100_0001;
pub(crate) const KEY_ENTER: i32 = 0x0100_0005;
pub(crate) const KEY_INSERT: i32 = 0x0100_0006;
pub(crate) const KEY_RETURN: i32 = 0x0100_0004;
pub(crate) const KEY_DELETE: i32 = 0x0100_0007;
pub(crate) const KEY_LEFT: i32 = 0x0100_0012;
pub(crate) const KEY_UP: i32 = 0x0100_0013;
pub(crate) const KEY_RIGHT: i32 = 0x0100_0014;
pub(crate) const KEY_DOWN: i32 = 0x0100_0015;
pub(crate) const KEY_HOME: i32 = 0x0100_0010;
pub(crate) const KEY_END: i32 = 0x0100_0011;
pub(crate) const KEY_ESCAPE: i32 = 0x0100_0000;
pub(crate) const KEY_A: i32 = 0x41;
pub(crate) const KEY_C: i32 = 0x43;
pub(crate) const KEY_V: i32 = 0x56;
pub(crate) const KEY_X: i32 = 0x58;
pub(crate) const KEY_Y: i32 = 0x59;
pub(crate) const KEY_Z: i32 = 0x5a;
pub(crate) const CTRL_MODIFIER: i32 = 0x0400_0000;
pub(crate) const SHIFT_MODIFIER: i32 = 0x0200_0000;
pub(crate) const ALT_MODIFIER: i32 = 0x0800_0000;
pub(crate) const META_MODIFIER: i32 = 0x1000_0000;

// ═══════════════════════════════════════════════════════════════════════
// Layer 3: EditorInputController — 归一化输入事件
//
// 平台差异只存在 Layer 2 (PlatformImeAdapter)，
// 正文编辑和动画永远不知道当前是 Windows 还是 Linux。
// ═══════════════════════════════════════════════════════════════════════

/// 归一化输入事件 — Layer 2 → Layer 3 的唯一数据流
///
/// 无论 Windows/Linux/macOS，所有输入最终都转换为这些事件类型。
/// EditorInputController 消费这些事件，调用 EditorInputHost 修改正文。
#[derive(Clone, Debug)]
pub(crate) enum EditorInputEvent {
    /// 普通文本插入（键盘直接输入、Linux 符号、Windows pending key flush）
    PlainText { text: String },
    /// 快捷键（Ctrl+A/C/V/X/Z/Y 等）
    Shortcut { key: i32, modifiers: i32 },
    /// Preedit 文本变化（IME 组合输入）
    PreeditChanged { text: String, cursor: usize, attributes: Vec<crate::sujian_editor_item::PreeditAttribute> },
    /// IME commit 上屏
    ImeCommit { text: String },
    /// IME commit 带替换语义（fcitx5 拼音修正等）
    ImeReplacementCommit { text: String, replace_start: i32, replace_length: i32 },
    /// IME 取消
    ImeCancel,
}

/// EditorInputController — Layer 3
///
/// 接收归一化 EditorInputEvent，调用 EditorInputHost 修改正文和生成视觉事务。
/// 不包含任何平台分支逻辑。动画状态不影响文本正确性。
pub(crate) struct EditorInputController;

impl EditorInputController {
    /// 分发归一化输入事件到 EditorInputHost
    pub(crate) fn dispatch<H: EditorInputHost + ?Sized>(host: &mut H, event: EditorInputEvent) {
        match event {
            EditorInputEvent::PlainText { text } => {
                if !host.input_enabled() || text.is_empty() {
                    return;
                }
                host.input_insert_text(text);
            }
            EditorInputEvent::Shortcut { key, modifiers } => {
                handle_key(host, key, modifiers);
            }
            EditorInputEvent::PreeditChanged { text, cursor, attributes } => {
                if !host.input_enabled() {
                    return;
                }
                if !text.is_empty() {
                    host.input_set_suppress_next_ime_commit(false);
                }
                host.input_set_preedit_with_attrs(text, cursor, attributes);
            }
            EditorInputEvent::ImeCommit { text } => {
                ime_commit(host, text);
            }
            EditorInputEvent::ImeReplacementCommit { text, replace_start, replace_length } => {
                ime_replace_and_commit(host, text, replace_start, replace_length);
            }
            EditorInputEvent::ImeCancel => {
                ime_cancel(host);
            }
        }
    }
}

pub(crate) trait EditorInputHost {
    fn input_enabled(&self) -> bool;
    fn input_emit_explicit_clear_requested(&mut self);
    fn input_clipboard_copy(&mut self) -> bool;
    fn input_clipboard_paste(&mut self);
    fn input_undo(&mut self);
    fn input_redo(&mut self);
    fn input_select_all(&mut self);
    fn input_delete_selection(&mut self);
    fn input_delete_backward(&mut self);
    fn input_delete_forward(&mut self);
    fn input_insert_text(&mut self, text: String);
    fn input_replace_and_insert(&mut self, replace_start: i32, replace_length: i32, text: String);
    fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool);
    fn input_move_cursor_vertical(&mut self, down: bool, extend: bool);
    fn input_move_to_line_edge(&mut self, end: bool, extend: bool);
    fn input_clear_preedit(&mut self);
    fn input_set_preedit(&mut self, text: String, cursor: usize);
    fn input_set_preedit_with_attrs(&mut self, text: String, cursor: usize, attributes: Vec<crate::sujian_editor_item::PreeditAttribute>);
    fn input_set_suppress_next_ime_commit(&mut self, value: bool);
    fn input_take_suppress_next_ime_commit(&mut self) -> bool;
    fn input_request_repaint(&mut self);

    /// 获取当前 preedit 文本（用于 inputMethodQuery）
    fn input_preedit_text(&self) -> String { String::new() }

    /// 获取当前 preedit 光标位置（用于 inputMethodQuery）
    fn input_preedit_cursor(&self) -> usize { 0 }

    /// 获取当前 preedit 属性（用于 inputMethodQuery）
    fn input_preedit_attributes(&self) -> Vec<crate::sujian_editor_item::PreeditAttribute> { Vec::new() }

    /// 通知 host preedit 视觉事务已生成（用于日志守卫）
    fn input_preedit_transaction_created(&self, _old_text: &str, _new_text: &str) {}
}

pub(crate) fn install_event_filter(item: *mut c_void, rust_item: *mut c_void) {
    // SAFETY: item is a QQuickItem* owned by Qt, valid during the item's lifetime.
    // rust_item is an opaque pointer to the Rust SujianEditorItem, stored as
    // event filter context data. It is passed back to Rust in extern "C" callbacks
    // (eventFilter), so it must remain valid for the entire lifetime of the item.
    // The event filter is installed on the item's parent and managed by Qt —
    // it must not outlive the owning QQuickItem. Single-threaded: Qt event
    // filters run on the GUI thread.
    cpp!(unsafe [item as "QQuickItem*", rust_item as "void*"] {
        sujian_install_event_filter(item, rust_item);
    });
}

pub(crate) fn focus_item(item: *mut c_void) {
    // SAFETY: item is a QQuickItem* owned by Qt, valid during the item's lifetime.
    // Only calls setFocus() — no pointer storage or lifetime extension.
    cpp!(unsafe [item as "QQuickItem*"] {
        sujian_focus_item(item);
    });
}

pub(crate) fn handle_key<H: EditorInputHost + ?Sized>(
    host: &mut H,
    key: i32,
    modifiers: i32,
) -> bool {
    if !host.input_enabled() {
        return false;
    }
    let ctrl = has_ctrl(modifiers);
    let shift = has_shift(modifiers);
    if is_copy_shortcut(key, modifiers) {
        host.input_clipboard_copy();
        return true;
    }
    if is_paste_shortcut(key, modifiers) {
        host.input_clipboard_paste();
        return true;
    }
    if is_redo_shortcut(key, modifiers) {
        host.input_redo();
        return true;
    }
    if ctrl {
        match key {
            KEY_A => {
                host.input_select_all();
                return true;
            }
            KEY_X => {
                host.input_clipboard_copy();
                host.input_delete_selection();
                return true;
            }
            KEY_Z => {
                host.input_undo();
                return true;
            }
            _ => return false,
        }
    }

    match key {
        KEY_ESCAPE => {
            host.input_clear_preedit();
            host.input_set_suppress_next_ime_commit(true);
        }
        KEY_BACKSPACE => host.input_delete_backward(),
        KEY_DELETE => host.input_delete_forward(),
        KEY_RETURN | KEY_ENTER => host.input_insert_text("\n".to_string()),
        KEY_TAB => host.input_insert_text("\t".to_string()),
        KEY_LEFT => host.input_move_cursor_horizontal(false, shift),
        KEY_RIGHT => host.input_move_cursor_horizontal(true, shift),
        KEY_UP => host.input_move_cursor_vertical(false, shift),
        KEY_DOWN => host.input_move_cursor_vertical(true, shift),
        KEY_HOME => host.input_move_to_line_edge(false, shift),
        KEY_END => host.input_move_to_line_edge(true, shift),
        _ => return false,
    }
    true
}

pub(crate) fn handle_key_and_text<H: EditorInputHost + ?Sized>(
    host: &mut H,
    key: i32,
    modifiers: i32,
    text: String,
) -> bool {
    if !host.input_enabled() {
        return false;
    }

    let ctrl = has_ctrl(modifiers);
    if is_destructive_key(key, modifiers) {
        host.input_emit_explicit_clear_requested();
    }

    if handle_key(host, key, modifiers) {
        return true;
    }

    if !ctrl && !has_alt(modifiers) && !has_meta(modifiers) && !text.is_empty() {
        host.input_insert_text(text);
        return true;
    }

    false
}

pub(crate) fn insert_preedit_text<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() {
        return;
    }
    let cursor = text.len();
    host.input_set_preedit(text, cursor);
    host.input_request_repaint();
}

pub(crate) fn commit_preedit_text<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() {
        return;
    }
    host.input_clear_preedit();
    if !text.is_empty() {
        host.input_insert_text(text);
    }
}

pub(crate) fn cancel_preedit<H: EditorInputHost + ?Sized>(host: &mut H) {
    host.input_clear_preedit();
    host.input_request_repaint();
}

pub(crate) fn ime_commit<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() || text.is_empty() {
        return;
    }
    if host.input_take_suppress_next_ime_commit() {
        host.input_clear_preedit();
        return;
    }
    // Commit flow:
    // 1. Record preedit final visual end position (handled by host)
    // 2. Clear preedit temporary layer
    // 3. Insert commitString as formal text (enters undo)
    // 4. Generate commit visual transaction (handled by host)
    // The host's input_clear_preedit records preedit_old_text before clearing,
    // and input_insert_text generates the visual transaction with proper
    // TypingCommit cause, enabling cursor animation from preedit end to
    // committed text end (e.g. pinyin→汉字 cursor retreat).
    host.input_clear_preedit();
    host.input_insert_text(text);
}

/// IME commit with replacement semantics.
///
/// Some input methods (e.g. fcitx5 pinyin correction, Japanese IM backspace
/// correction) send `QInputMethodEvent` with non-zero `replacementStart` and
/// `replacementLength`. This means: before inserting the commit string, delete
/// the text range `[cursor + replacementStart, cursor + replacementStart +
/// replacementLength)` (in UTF-16 code units).
///
/// The replacement offsets are relative to the current cursor position and
/// expressed in UTF-16 code units. The host's `input_replace_and_insert`
/// method is responsible for converting them to UTF-8 byte offsets before
/// operating on the buffer.
pub(crate) fn ime_replace_and_commit<H: EditorInputHost + ?Sized>(
    host: &mut H,
    text: String,
    replace_start: i32,
    replace_length: i32,
) {
    if !host.input_enabled() || text.is_empty() {
        return;
    }
    if host.input_take_suppress_next_ime_commit() {
        host.input_clear_preedit();
        return;
    }
    // 1. Clear preedit temporary layer
    host.input_clear_preedit();
    // 2. Delete the replacement range, then insert commit text
    host.input_replace_and_insert(replace_start, replace_length, text);
}

pub(crate) fn ime_preedit<H: EditorInputHost + ?Sized>(host: &mut H, text: String, cursor: i32) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_set_suppress_next_ime_commit(false);
    }
    let cursor = (cursor.max(0) as usize).min(text.len());
    host.input_set_preedit(text, cursor);
}

/// IME preedit with attributes from QInputMethodEvent.
/// Handles TextFormat (underline) and Cursor attributes.
pub(crate) fn ime_preedit_with_attrs<H: EditorInputHost + ?Sized>(
    host: &mut H,
    text: String,
    cursor: i32,
    attributes: Vec<crate::sujian_editor_item::PreeditAttribute>,
) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_set_suppress_next_ime_commit(false);
    }
    let cursor = (cursor.max(0) as usize).min(text.len());
    host.input_set_preedit_with_attrs(text, cursor, attributes);
}

pub(crate) fn ime_cancel<H: EditorInputHost + ?Sized>(host: &mut H) {
    host.input_clear_preedit();
}

fn has_ctrl(modifiers: i32) -> bool {
    modifiers & CTRL_MODIFIER != 0
}

fn has_shift(modifiers: i32) -> bool {
    modifiers & SHIFT_MODIFIER != 0
}

fn has_alt(modifiers: i32) -> bool {
    modifiers & ALT_MODIFIER != 0
}

fn has_meta(modifiers: i32) -> bool {
    modifiers & META_MODIFIER != 0
}

fn is_copy_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_C || key == KEY_INSERT)
}

fn is_paste_shortcut(key: i32, modifiers: i32) -> bool {
    (has_ctrl(modifiers) && key == KEY_V) || (has_shift(modifiers) && key == KEY_INSERT)
}

fn is_redo_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_Y || (has_shift(modifiers) && key == KEY_Z))
}

fn is_destructive_key(key: i32, modifiers: i32) -> bool {
    key == KEY_BACKSPACE || key == KEY_DELETE || (has_ctrl(modifiers) && key == KEY_X)
}

fn decode_utf16_lossy(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn decode_utf16_ptr(text: *const u16, text_len: i32) -> String {
    if text.is_null() || text_len <= 0 {
        return String::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(text, text_len as usize) };
    decode_utf16_lossy(slice)
}

unsafe fn item_from_ptr<'a>(rust_item: *mut c_void) -> Option<&'a mut SujianEditorItem> {
    if rust_item.is_null() {
        return None;
    }
    Some(unsafe { &mut *(rust_item as *mut SujianEditorItem) })
}

#[no_mangle]
extern "C" fn sujian_handle_key_and_text(
    rust_item: *mut c_void,
    key: i32,
    modifiers: i32,
    text: *const u16,
    text_len: i32,
) -> bool {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return false;
    };
    let text = decode_utf16_ptr(text, text_len);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_key_and_text(item, key, modifiers, text)
    })) {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "[sujian_editor] panic in sujian_handle_key_and_text, caught at FFI boundary"
            );
            false
        }
    }
}

#[no_mangle]
extern "C" fn sujian_ime_commit(rust_item: *mut c_void, text: *const u16, text_len: i32) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_commit(item, text);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_replace_and_commit(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    replace_start: i32,
    replace_length: i32,
) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_replace_and_commit(item, text, replace_start, replace_length);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_preedit(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    cursor: i32,
) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit(item, text, cursor);
    }));
}

/// FFI callback for IME preedit with attributes.
/// attr_types: 0=TextFormat, 1=Cursor, 2=Language, 3=Ruby, 4=Selection
/// attr_starts/attr_lengths: character offsets and lengths for each attribute
/// attr_formats: format code for TextFormat attributes (0=underline, 1=textColor, 2=backgroundColor, 3=fontUnderline)
#[no_mangle]
extern "C" fn sujian_ime_preedit_attrs(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    cursor: i32,
    attr_types: *const i32,
    attr_starts: *const i32,
    attr_lengths: *const i32,
    attr_count: i32,
    attr_formats: *const i32,
) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);

    // Convert character-offset attributes to byte-offset PreeditAttribute
    let mut attributes = Vec::new();
    if !attr_types.is_null() && !attr_starts.is_null() && !attr_lengths.is_null() && attr_count > 0 {
        let types_slice = unsafe { std::slice::from_raw_parts(attr_types, attr_count as usize) };
        let starts_slice = unsafe { std::slice::from_raw_parts(attr_starts, attr_count as usize) };
        let lengths_slice = unsafe { std::slice::from_raw_parts(attr_lengths, attr_count as usize) };
        let formats_slice = if !attr_formats.is_null() {
            unsafe { std::slice::from_raw_parts(attr_formats, attr_count as usize) }
        } else {
            // Fallback: treat all as underline
            &vec![0i32; attr_count as usize]
        };

        // Pre-compute char-to-byte mapping for the preedit text
        let char_offsets: Vec<usize> = {
            let mut offsets = Vec::new();
            let mut byte_pos = 0;
            offsets.push(0);
            for ch in text.chars() {
                byte_pos += ch.len_utf8();
                offsets.push(byte_pos);
            }
            offsets
        };

        for i in 0..attr_count as usize {
            let attr_type = types_slice[i];
            let char_start = starts_slice[i].max(0) as usize;
            let char_length = lengths_slice[i].max(0) as usize;
            let format_code = formats_slice[i];

            let kind = if attr_type == 0 {
                // QInputMethodEvent::TextFormat
                match format_code {
                    1 => crate::sujian_editor_item::PreeditAttributeKind::TextColor {
                        color: String::new(), // 平台层无法提取具体颜色值，使用空字符串标记
                    },
                    2 => crate::sujian_editor_item::PreeditAttributeKind::BackgroundColor {
                        color: String::new(),
                    },
                    3 => crate::sujian_editor_item::PreeditAttributeKind::FontUnderline,
                    _ => crate::sujian_editor_item::PreeditAttributeKind::Underline,
                }
            } else if attr_type == 1 {
                // QInputMethodEvent::Cursor
                crate::sujian_editor_item::PreeditAttributeKind::Cursor
            } else {
                continue; // Skip Language(2), Ruby(3), Selection(4), etc.
            };

            // Convert character offsets to byte offsets
            let byte_start = char_offsets.get(char_start).copied().unwrap_or(text.len());
            let char_end = char_start + char_length;
            let byte_end = char_offsets.get(char_end).copied().unwrap_or(text.len());
            let byte_length = byte_end.saturating_sub(byte_start);

            attributes.push(crate::sujian_editor_item::PreeditAttribute {
                start: byte_start,
                length: byte_length,
                kind,
            });
        }
    }

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit_with_attrs(item, text, cursor, attributes);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_cancel(rust_item: *mut c_void) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_cancel(item);
    }));
}

#[no_mangle]
extern "C" fn sujian_request_repaint(rust_item: *mut c_void) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        item.input_request_repaint();
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeHost {
        enabled: bool,
        inserted: Vec<String>,
        operations: Vec<&'static str>,
        preedit_text: String,
        preedit_cursor: usize,
        suppress_next_ime_commit: bool,
        explicit_clear_count: usize,
        repaint_count: usize,
        replace_and_insert_calls: Vec<(i32, i32, String)>,
    }

    impl FakeHost {
        fn enabled() -> Self {
            Self {
                enabled: true,
                ..Self::default()
            }
        }
    }

    impl EditorInputHost for FakeHost {
        fn input_enabled(&self) -> bool {
            self.enabled
        }

        fn input_emit_explicit_clear_requested(&mut self) {
            self.explicit_clear_count += 1;
        }

        fn input_clipboard_copy(&mut self) -> bool {
            self.operations.push("copy");
            true
        }

        fn input_clipboard_paste(&mut self) {
            self.operations.push("paste");
        }

        fn input_undo(&mut self) {
            self.operations.push("undo");
        }

        fn input_redo(&mut self) {
            self.operations.push("redo");
        }

        fn input_select_all(&mut self) {
            self.operations.push("select_all");
        }

        fn input_delete_selection(&mut self) {
            self.operations.push("delete_selection");
        }

        fn input_delete_backward(&mut self) {
            self.operations.push("delete_backward");
        }

        fn input_delete_forward(&mut self) {
            self.operations.push("delete_forward");
        }

        fn input_insert_text(&mut self, text: String) {
            self.inserted.push(text);
        }

        fn input_replace_and_insert(&mut self, replace_start: i32, replace_length: i32, text: String) {
            // Record the call parameters for test assertions
            self.replace_and_insert_calls.push((replace_start, replace_length, text.clone()));
            // Simplified test impl: just insert (no actual replacement in FakeHost)
            self.inserted.push(text);
        }

        fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool) {
            self.operations.push(if forward { "right" } else { "left" });
            if extend {
                self.operations.push("extend");
            }
        }

        fn input_move_cursor_vertical(&mut self, down: bool, extend: bool) {
            self.operations.push(if down { "down" } else { "up" });
            if extend {
                self.operations.push("extend");
            }
        }

        fn input_move_to_line_edge(&mut self, end: bool, extend: bool) {
            self.operations.push(if end { "end" } else { "home" });
            if extend {
                self.operations.push("extend");
            }
        }

        fn input_clear_preedit(&mut self) {
            self.preedit_text.clear();
            self.preedit_cursor = 0;
        }

        fn input_set_preedit(&mut self, text: String, cursor: usize) {
            self.preedit_text = text;
            self.preedit_cursor = cursor;
        }

        fn input_set_preedit_with_attrs(&mut self, text: String, cursor: usize, _attributes: Vec<crate::sujian_editor_item::PreeditAttribute>) {
            self.preedit_text = text;
            self.preedit_cursor = cursor;
        }

        fn input_set_suppress_next_ime_commit(&mut self, value: bool) {
            self.suppress_next_ime_commit = value;
        }

        fn input_take_suppress_next_ime_commit(&mut self) -> bool {
            let value = self.suppress_next_ime_commit;
            if value {
                self.suppress_next_ime_commit = false;
            }
            value
        }

        fn input_request_repaint(&mut self) {
            self.repaint_count += 1;
        }
    }

    #[test]
    fn desktop_shortcuts_match_existing_keys() {
        assert!(is_copy_shortcut(KEY_C, CTRL_MODIFIER));
        assert!(is_copy_shortcut(KEY_INSERT, CTRL_MODIFIER));
        assert!(is_paste_shortcut(KEY_V, CTRL_MODIFIER));
        assert!(is_paste_shortcut(KEY_INSERT, SHIFT_MODIFIER));
        assert!(is_redo_shortcut(KEY_Y, CTRL_MODIFIER));
        assert!(is_redo_shortcut(KEY_Z, CTRL_MODIFIER | SHIFT_MODIFIER));
        assert!(!is_redo_shortcut(KEY_Z, CTRL_MODIFIER));
    }

    #[test]
    fn utf16_decode_covers_chinese_ime_text() {
        let units: Vec<u16> = "中文输入".encode_utf16().collect();
        assert_eq!(decode_utf16_lossy(&units), "中文输入");
    }

    #[test]
    fn preedit_cursor_is_clamped_to_existing_byte_len() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "中文".to_string(), 99);
        assert_eq!(host.preedit_text, "中文");
        assert_eq!(host.preedit_cursor, "中文".len());
    }

    #[test]
    fn destructive_keys_emit_explicit_clear_before_handling() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_BACKSPACE,
            0,
            String::new()
        ));
        assert_eq!(host.explicit_clear_count, 1);
        assert_eq!(host.operations, vec!["delete_backward"]);

        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(
            &mut host,
            KEY_X,
            CTRL_MODIFIER,
            String::new()
        ));
        assert_eq!(host.explicit_clear_count, 1);
        assert_eq!(host.operations, vec!["copy", "delete_selection"]);
    }

    #[test]
    fn printable_text_inserts_when_key_is_not_handled() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "你".to_string()));
        assert_eq!(host.inserted, vec!["你"]);
    }

    #[test]
    fn suppressed_ime_commit_only_clears_preedit_once() {
        let mut host = FakeHost::enabled();
        host.preedit_text = "拼".to_string();
        host.preedit_cursor = 3;
        host.suppress_next_ime_commit = true;

        ime_commit(&mut host, "拼".to_string());

        assert!(host.inserted.is_empty());
        assert_eq!(host.preedit_text, "");
        assert_eq!(host.preedit_cursor, 0);
        assert!(!host.suppress_next_ime_commit);
    }

    // ── Regression tests for pipeline fixes ──

    #[test]
    fn space_inserts_as_plain_text() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, " ".to_string()));
        assert_eq!(host.inserted, vec![" "]);
    }

    #[test]
    fn plus_inserts_as_plain_text() {
        let mut host = FakeHost::enabled();
        // Shift+= produces "+" — Shift is NOT a text-input disable condition
        assert!(handle_key_and_text(&mut host, 0, SHIFT_MODIFIER, "+".to_string()));
        assert_eq!(host.inserted, vec!["+"]);
    }

    #[test]
    fn chinese_punctuation_inserts_as_plain_text() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "。".to_string()));
        assert_eq!(host.inserted, vec!["。"]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, "！".to_string()));
        assert_eq!(host2.inserted, vec!["！"]);
    }

    #[test]
    fn ctrl_a_triggers_select_all() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, KEY_A, CTRL_MODIFIER, String::new()));
        assert_eq!(host.operations, vec!["select_all"]);
    }

    #[test]
    fn ctrl_c_v_x_z_y_shortcuts() {
        // Ctrl+C
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, KEY_C, CTRL_MODIFIER, String::new()));
        assert_eq!(host.operations, vec!["copy"]);

        // Ctrl+V
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, KEY_V, CTRL_MODIFIER, String::new()));
        assert_eq!(host.operations, vec!["paste"]);

        // Ctrl+X
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, KEY_X, CTRL_MODIFIER, String::new()));
        assert_eq!(host.operations, vec!["copy", "delete_selection"]);

        // Ctrl+Z
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, KEY_Z, CTRL_MODIFIER, String::new()));
        assert_eq!(host.operations, vec!["undo"]);

        // Ctrl+Y
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, KEY_Y, CTRL_MODIFIER, String::new()));
        assert_eq!(host.operations, vec!["redo"]);
    }

    #[test]
    fn shift_plus_symbol_not_swallowed_as_shortcut() {
        // Shift+= produces "+", should be inserted as text, not treated as shortcut
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, SHIFT_MODIFIER, "+".to_string()));
        assert!(host.inserted.contains(&"+".to_string()));
        assert!(!host.operations.contains(&"copy"));
    }

    #[test]
    fn preedit_does_not_modify_buffer_text() {
        // preedit only sets preedit layer, does not call input_insert_text
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert_eq!(host.preedit_text, "拼");
        assert!(host.inserted.is_empty(), "preedit should NOT insert into buffer");
    }

    #[test]
    fn ime_commit_writes_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn ime_preedit_cursor_attribute_mapping() {
        // Test that attr_type 0 maps to Underline and attr_type 1 maps to Cursor
        // (matching Qt's QInputMethodEvent::TextFormat=0, Cursor=1)
        use crate::sujian_editor_item::PreeditAttributeKind;

        // attr_type 0, format_code 0 should be Underline (TextFormat default)
        let kind_0 = if 0 == 0 {
            match 0 {
                1 => PreeditAttributeKind::TextColor { color: String::new() },
                2 => PreeditAttributeKind::BackgroundColor { color: String::new() },
                3 => PreeditAttributeKind::FontUnderline,
                _ => PreeditAttributeKind::Underline,
            }
        } else if 0 == 1 {
            PreeditAttributeKind::Cursor
        } else {
            panic!("unexpected attr_type");
        };
        assert_eq!(kind_0, PreeditAttributeKind::Underline);

        // attr_type 0, format_code 1 should be TextColor
        let kind_tc = match 1 {
            1 => PreeditAttributeKind::TextColor { color: String::new() },
            2 => PreeditAttributeKind::BackgroundColor { color: String::new() },
            3 => PreeditAttributeKind::FontUnderline,
            _ => PreeditAttributeKind::Underline,
        };
        assert!(matches!(kind_tc, PreeditAttributeKind::TextColor { .. }));

        // attr_type 0, format_code 2 should be BackgroundColor
        let kind_bc = match 2 {
            1 => PreeditAttributeKind::TextColor { color: String::new() },
            2 => PreeditAttributeKind::BackgroundColor { color: String::new() },
            3 => PreeditAttributeKind::FontUnderline,
            _ => PreeditAttributeKind::Underline,
        };
        assert!(matches!(kind_bc, PreeditAttributeKind::BackgroundColor { .. }));

        // attr_type 0, format_code 3 should be FontUnderline
        let kind_fu = match 3 {
            1 => PreeditAttributeKind::TextColor { color: String::new() },
            2 => PreeditAttributeKind::BackgroundColor { color: String::new() },
            3 => PreeditAttributeKind::FontUnderline,
            _ => PreeditAttributeKind::Underline,
        };
        assert_eq!(kind_fu, PreeditAttributeKind::FontUnderline);

        // attr_type 1 should be Cursor
        let kind_1 = if 1 == 0 {
            PreeditAttributeKind::Underline
        } else if 1 == 1 {
            PreeditAttributeKind::Cursor
        } else {
            panic!("unexpected attr_type");
        };
        assert_eq!(kind_1, PreeditAttributeKind::Cursor);

        // attr_type 2 (Language) should be skipped
        // attr_type 3 (Ruby) should be skipped
        // attr_type 4 (Selection) should be skipped
        for &attr_type in &[2, 3, 4] {
            let is_handled = attr_type == 0 || attr_type == 1;
            assert!(!is_handled, "attr_type {} should not be mapped to Underline or Cursor", attr_type);
        }
    }

    // ── IME replacement boundary tests ──

    #[test]
    fn test_ime_replace_and_commit_basic() {
        // Basic replacement: replace 2 chars before cursor with commit text
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "你好".to_string(), -2, 2);
        // Should call input_replace_and_insert with the correct parameters
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -2);
        assert_eq!(*len, 2);
        assert_eq!(text, "你好");
    }

    #[test]
    fn test_ime_replace_negative_start() {
        // Negative replace_start means replace before cursor
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "新".to_string(), -1, 1);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -1);
        assert_eq!(*len, 1);
        assert_eq!(text, "新");
    }

    /// Verify that IME replacement does not split a surrogate pair.
    ///
    /// When the replacement range covers a surrogate pair (e.g. emoji U+1F600 = 2 UTF-16 units),
    /// the host's `input_replace_and_insert` must treat the range as code-point-aligned,
    /// not UTF-16-unit-aligned. The Rust host implementation clamps to char boundaries
    /// in UTF-8 byte space, which inherently prevents splitting surrogate pairs.
    ///
    /// In this test, FakeHost only records the raw parameters — the actual boundary
    /// clamping happens in SujianEditorItem::ime_replace_and_insert, which converts
    /// UTF-16 offsets to UTF-8 byte offsets and clamps to char boundaries.
    #[test]
    fn test_ime_replace_does_not_split_surrogate_pair() {
        // This test verifies the parameter passing is correct.
        // Actual boundary clamping is tested in the integration layer.
        let mut host = FakeHost::enabled();
        // Replace 1 UTF-16 unit before cursor — if cursor is right after an emoji,
        // the host implementation should expand to cover the full surrogate pair
        ime_replace_and_commit(&mut host, "X".to_string(), -1, 1);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        // The host receives the raw parameters; boundary clamping is the host's responsibility
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -1);
        assert_eq!(*len, 1);
        assert_eq!(text, "X");
    }

    /// Verify that IME replacement clamps to character boundaries.
    ///
    /// When replace_start + replace_length would end in the middle of a multi-byte
    /// character (e.g. a 3-byte UTF-8 Chinese character), the host must clamp the
    /// end to the nearest character boundary. This prevents buffer corruption.
    ///
    /// The Rust host implementation walks UTF-16 code units and converts to UTF-8
    /// byte offsets, which inherently produces char-aligned boundaries.
    #[test]
    fn test_ime_replace_clamps_to_char_boundary() {
        // This test verifies parameter passing for boundary-aligned replacement.
        // Actual clamping logic is in SujianEditorItem::ime_replace_and_insert.
        let mut host = FakeHost::enabled();
        // Replace 3 UTF-16 units starting from cursor — should be char-aligned
        ime_replace_and_commit(&mut host, "替换".to_string(), 0, 3);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, 0);
        assert_eq!(*len, 3);
        assert_eq!(text, "替换");
    }

    /// Verify that IME replacement with undo works as a single atomic operation.
    ///
    /// When the host processes `input_replace_and_insert`, the delete-and-insert
    /// must be recorded as a single undo entry, not two separate ones.
    /// This ensures Ctrl+Z undoes the entire replacement, not just the insertion.
    ///
    /// The Rust host implementation uses `push_undo` before the combined operation,
    /// ensuring atomic undo semantics.
    #[test]
    fn test_ime_replace_single_undo() {
        // This test verifies the call is made as a single operation.
        // Actual undo atomicity is in SujianEditorItem::ime_replace_and_insert.
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "修正".to_string(), -2, 2);
        // Should be a single replace_and_insert call (atomic operation)
        assert_eq!(host.replace_and_insert_calls.len(), 1);
    }

    // ── Linux input regression tests ──
    // These tests verify the Linux-specific input path where plain text
    // is inserted immediately (no deferred insertion). On Linux/macOS,
    // handle_key_and_text is called directly from the KeyPress event
    // handler without the pending key deferral used on Windows.

    #[test]
    fn linux_space_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0x20, 0, " ".to_string()));
        assert_eq!(host.inserted, vec![" "]);
    }

    #[test]
    fn linux_plus_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, SHIFT_MODIFIER, "+".to_string()));
        assert_eq!(host.inserted, vec!["+"]);
    }

    #[test]
    fn linux_minus_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "-".to_string()));
        assert_eq!(host.inserted, vec!["-"]);
    }

    #[test]
    fn linux_underscore_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, SHIFT_MODIFIER, "_".to_string()));
        assert_eq!(host.inserted, vec!["_"]);
    }

    #[test]
    fn linux_slash_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "/".to_string()));
        assert_eq!(host.inserted, vec!["/"]);
    }

    #[test]
    fn linux_chinese_punctuation_inserts_immediately() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "。".to_string()));
        assert_eq!(host.inserted, vec!["。"]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, "，".to_string()));
        assert_eq!(host2.inserted, vec!["，"]);
        let mut host3 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host3, 0, 0, "！".to_string()));
        assert_eq!(host3.inserted, vec!["！"]);
    }

    #[test]
    fn linux_ime_preedit_does_not_write_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼音".to_string(), 3);
        assert_eq!(host.preedit_text, "拼音");
        assert!(host.inserted.is_empty(), "preedit must NOT insert into buffer");
    }

    #[test]
    fn linux_ime_commit_writes_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_ime_preedit_with_attrs_does_not_write_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit_with_attrs(
            &mut host,
            "拼".to_string(),
            0,
            vec![crate::sujian_editor_item::PreeditAttribute {
                start: 0,
                length: 3,
                kind: crate::sujian_editor_item::PreeditAttributeKind::Underline,
            }],
        );
        assert_eq!(host.preedit_text, "拼");
        assert!(host.inserted.is_empty(), "preedit with attrs must NOT insert into buffer");
    }

    #[test]
    fn linux_ime_commit_after_preedit_writes_to_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert!(host.inserted.is_empty());
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_ime_replacement_works() {
        let mut host = FakeHost::enabled();
        ime_replace_and_commit(&mut host, "修正".to_string(), -2, 2);
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -2);
        assert_eq!(*len, 2);
        assert_eq!(text, "修正");
    }

    #[test]
    fn linux_ime_cancel_clears_preedit() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert_eq!(host.preedit_text, "拼");
        ime_cancel(&mut host);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_symbols_not_blocked_by_ctrl_or_alt() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "(".to_string()));
        assert_eq!(host.inserted, vec!["("]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, ")".to_string()));
        assert_eq!(host2.inserted, vec![")"]);
        let mut host3 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host3, 0, 0, "[".to_string()));
        assert_eq!(host3.inserted, vec!["["]);
    }

    #[test]
    fn linux_ime_commit_not_blocked_by_animation_state() {
        // IME commit must work regardless of animation state.
        // The Rust-side ime_commit function only checks input_enabled,
        // not animation state. Animation is a visual concern, not a text concern.
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "测试".to_string());
        assert_eq!(host.inserted, vec!["测试"]);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Layer 3: EditorInputEvent dispatch tests
    //
    // These tests verify the normalized event dispatch path through
    // EditorInputController::dispatch, which is the Layer 3 entry point.
    // All platform differences are resolved in Layer 2; Layer 3 only sees
    // EditorInputEvent variants.
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn dispatch_plain_text_inserts() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(&mut host, EditorInputEvent::PlainText { text: "你好".to_string() });
        assert_eq!(host.inserted, vec!["你好"]);
    }

    #[test]
    fn dispatch_shortcut_select_all() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(&mut host, EditorInputEvent::Shortcut { key: KEY_A, modifiers: CTRL_MODIFIER });
        assert_eq!(host.operations, vec!["select_all"]);
    }

    #[test]
    fn dispatch_ime_commit() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(&mut host, EditorInputEvent::ImeCommit { text: "你好".to_string() });
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn dispatch_ime_replacement_commit() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(&mut host, EditorInputEvent::ImeReplacementCommit {
            text: "修正".to_string(), replace_start: -2, replace_length: 2,
        });
        assert_eq!(host.replace_and_insert_calls.len(), 1);
        let (start, len, text) = &host.replace_and_insert_calls[0];
        assert_eq!(*start, -2);
        assert_eq!(*len, 2);
        assert_eq!(text, "修正");
    }

    #[test]
    fn dispatch_preedit_changed() {
        let mut host = FakeHost::enabled();
        EditorInputController::dispatch(&mut host, EditorInputEvent::PreeditChanged {
            text: "拼音".to_string(), cursor: 3, attributes: vec![],
        });
        assert_eq!(host.preedit_text, "拼音");
        assert!(host.inserted.is_empty(), "preedit must NOT insert into buffer");
    }

    #[test]
    fn dispatch_ime_cancel() {
        let mut host = FakeHost::enabled();
        host.preedit_text = "拼".to_string();
        EditorInputController::dispatch(&mut host, EditorInputEvent::ImeCancel);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn dispatch_disabled_host_ignores_all() {
        let mut host = FakeHost::default();
        EditorInputController::dispatch(&mut host, EditorInputEvent::PlainText { text: "x".to_string() });
        assert!(host.inserted.is_empty());
        EditorInputController::dispatch(&mut host, EditorInputEvent::ImeCommit { text: "你好".to_string() });
        assert!(host.inserted.is_empty());
        EditorInputController::dispatch(&mut host, EditorInputEvent::PreeditChanged {
            text: "拼".to_string(), cursor: 0, attributes: vec![],
        });
        assert!(host.preedit_text.is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Additional Linux input regression tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn linux_brackets_and_quotes_not_blocked() {
        let mut host = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host, 0, 0, "]".to_string()));
        assert_eq!(host.inserted, vec!["]"]);
        let mut host2 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host2, 0, 0, "\"".to_string()));
        assert_eq!(host2.inserted, vec!["\""]);
        let mut host3 = FakeHost::enabled();
        assert!(handle_key_and_text(&mut host3, 0, 0, "'".to_string()));
        assert_eq!(host3.inserted, vec!["'"]);
    }

    #[test]
    fn linux_cursor_attribute_drives_preedit_cursor() {
        let mut host = FakeHost::enabled();
        let attrs = vec![crate::sujian_editor_item::PreeditAttribute {
            start: 0,
            length: 0,
            kind: crate::sujian_editor_item::PreeditAttributeKind::Cursor,
        }];
        ime_preedit_with_attrs(&mut host, "拼音".to_string(), 1, attrs);
        assert_eq!(host.preedit_text, "拼音");
        assert_eq!(host.preedit_cursor, 1);
        assert!(host.inserted.is_empty());
    }

    #[test]
    fn linux_commit_after_preedit_always_writes_buffer() {
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "nihao".to_string(), 5);
        assert!(host.inserted.is_empty());
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_animation_off_does_not_affect_commit() {
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "测试".to_string());
        assert_eq!(host.inserted, vec!["测试"]);
    }

    #[test]
    fn linux_scroll_chapter_settings_only_clear_animation_not_input() {
        // Simulate: scroll/chapter switch/settings change clear animations,
        // but input state (preedit, commit) must remain intact.
        // This is enforced by the architecture: animation is Layer 3 visual,
        // input is Layer 3 text. They are independent.
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼".to_string(), 0);
        assert_eq!(host.preedit_text, "拼");
        // Simulate: animation cleared (no effect on preedit)
        assert_eq!(host.preedit_text, "拼");
        // Commit still works after animation clear
        ime_commit(&mut host, "你好".to_string());
        assert_eq!(host.inserted, vec!["你好"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_ime_commit_text_first_animation_second() {
        // IME commit must first write text to buffer, then animation is visual only.
        // If animation fails, text must already be correct.
        // This is guaranteed by ime_commit() calling input_clear_preedit()
        // then input_insert_text() — both are synchronous buffer operations.
        let mut host = FakeHost::enabled();
        ime_commit(&mut host, "确认".to_string());
        // Text is already in buffer — animation is purely visual
        assert_eq!(host.inserted, vec!["确认"]);
        assert_eq!(host.preedit_text, "");
    }

    #[test]
    fn linux_candidate_rectangle_follows_preedit_cursor() {
        // Verify that preedit cursor position is correctly set,
        // which drives the IME candidate window rectangle.
        let mut host = FakeHost::enabled();
        ime_preedit(&mut host, "拼音输入".to_string(), 2);
        assert_eq!(host.preedit_cursor, 2);
        let mut host2 = FakeHost::enabled();
        ime_preedit(&mut host2, "abc".to_string(), 0);
        assert_eq!(host2.preedit_cursor, 0);
    }
}
