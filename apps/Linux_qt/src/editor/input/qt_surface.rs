//! Layer 1: QtInputSurface (SujianEventFilter)
//!
//! Qt 官方事件入口：keyPressEvent / inputMethodEvent / inputMethodQuery
//! 不写正文业务，不写动画逻辑
//! 委托给 Linux PlatformImeAdapter 处理 fcitx5/ibus 语义
//!
//! 此文件包含 C++ PlatformImeAdapter 类和 SujianEventFilter 类定义。
//! cpp! 宏将所有 cpp! 块合并到同一编译单元，类定义只能出现在一个 Rust 文件中。

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
    #include <QMetaMethod>

    extern "C" bool sujian_handle_key_and_text(void* rust_item, int key, int modifiers, const ushort* text, int text_len);
    extern "C" void sujian_ime_commit(void* rust_item, const ushort* text, int text_len);
    extern "C" void sujian_ime_replace_and_commit(void* rust_item, const ushort* text, int text_len, int replace_start, int replace_length);
    extern "C" void sujian_ime_preedit(void* rust_item, const ushort* text, int text_len, int cursor);
    extern "C" void sujian_ime_preedit_attrs(void* rust_item, const ushort* text, int text_len, int cursor, const int* attr_types, const int* attr_starts, const int* attr_lengths, int attr_count, const int* attr_formats);
    extern "C" void sujian_ime_cancel(void* rust_item);
    extern "C" void sujian_request_repaint(void* rust_item);

    // PlatformImeAdapter is defined by editor/input/platform/linux/input_surface.rs.
    // Keep this file as QtInputSurface only so Linux IME policy stays isolated.

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
            // Linux/fcitx/ibus deliver InputMethodEvent with Linux Qt semantics;
            // plain text keys are inserted immediately. Do NOT use
            // QInputMethod::isVisible() — it is unreliable and blocks space, +,
            // and other symbols from being inserted.
            if (is_plain_text_key(ke)) {
                bool accepted = sujian_handle_key_and_text(
                    rust_item,
                    static_cast<int>(ke->key()),
                    static_cast<int>(ke->modifiers()),
                    reinterpret_cast<const ushort*>(ke->text().utf16()),
                    static_cast<int>(ke->text().size())
                );
                return accepted;
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
                && ime_adapter.can_accept_plain_text_key();
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
                       ime_adapter.platform_name());
            }
            if (!commit.isEmpty()) {
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
                sujian_ime_cancel(rust_item);
                ime_adapter.ime_composing = false;
            }
            sujian_request_repaint(rust_item);
            QInputMethod* im = QGuiApplication::inputMethod();
            if (im) {
                im->update(Qt::ImCursorRectangle | Qt::ImAnchorRectangle | Qt::ImCursorPosition | Qt::ImSurroundingText | Qt::ImCurrentSelection);
            }
            ime->accept();
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
                QVariant anchorXVar = obj->property("anchor_rect_x");
                if (anchorXVar.isValid()) {
                    double ax = anchorXVar.toDouble();
                    double ay = obj->property("anchor_rect_y").toDouble();
                    double aw = obj->property("anchor_rect_width").toDouble();
                    double ah = obj->property("anchor_rect_height").toDouble();
                    qe->setValue(Qt::ImAnchorRectangle, QRectF(ax, ay, aw, ah));
                } else {
                    double cx = obj->property("cursor_rect_x").toDouble();
                    double cy = obj->property("cursor_rect_y").toDouble();
                    double cw = obj->property("cursor_rect_width").toDouble();
                    double ch = obj->property("cursor_rect_height").toDouble();
                    qe->setValue(Qt::ImAnchorRectangle, QRectF(cx, cy, cw, ch));
                }
            }
            if (qe->queries() & Qt::ImSurroundingText) {
                QString fullText = obj->property("plain_text").toString();
                int cursorPos = obj->property("cursor_position").toInt();
                int beforeLen = qMin(cursorPos, 100);
                int afterLen = qMin(fullText.length() - cursorPos, 100);
                QString surrounding = fullText.mid(cursorPos - beforeLen, beforeLen + afterLen);
                qe->setValue(Qt::ImSurroundingText, surrounding);
            }
            if (qe->queries() & Qt::ImCursorPosition) {
                int cursorPos = obj->property("cursor_position").toInt();
                int beforeLen = qMin(cursorPos, 100);
                qe->setValue(Qt::ImCursorPosition, beforeLen);
            }
            if (qe->queries() & Qt::ImCurrentSelection) {
                QString selText = obj->property("current_selection_text").toString();
                qe->setValue(Qt::ImCurrentSelection, selText);
            }
            if (qe->queries() & Qt::ImAbsolutePosition) {
                double cx = obj->property("cursor_rect_x").toDouble();
                double cy = obj->property("cursor_rect_y").toDouble();
                QQuickItem* quickItem = qobject_cast<QQuickItem*>(obj);
                if (quickItem) {
                    QPointF scenePos = quickItem->mapToScene(QPointF(cx, cy));
                    qe->setValue(Qt::ImAbsolutePosition, scenePos);
                }
            }
            if (qe->queries() & Qt::ImAnchorPosition) {
                int cursorPos = obj->property("cursor_position").toInt();
                QString selText = obj->property("current_selection_text").toString();
                if (!selText.isEmpty()) {
                    int anchorPos = cursorPos + selText.length();
                    qe->setValue(Qt::ImAnchorPosition, anchorPos);
                } else {
                    qe->setValue(Qt::ImAnchorPosition, cursorPos);
                }
            }
            if (qe->queries() & Qt::ImTextBeforeCursor) {
                QString fullText = obj->property("plain_text").toString();
                int cursorPos = obj->property("cursor_position").toInt();
                int beforeLen = qMin(cursorPos, 100);
                qe->setValue(Qt::ImTextBeforeCursor, fullText.mid(cursorPos - beforeLen, beforeLen));
            }
            if (qe->queries() & Qt::ImTextAfterCursor) {
                QString fullText = obj->property("plain_text").toString();
                int cursorPos = obj->property("cursor_position").toInt();
                int afterLen = qMin(fullText.length() - cursorPos, 100);
                qe->setValue(Qt::ImTextAfterCursor, fullText.mid(cursorPos, afterLen));
            }
            if (qEnvironmentVariableIsSet("SUJIAN_EDITOR_DEBUG") || qEnvironmentVariableIsSet("WRITER_DEBUG")) {
                qDebug("[sujian] InputMethodQuery: queries=0x%x", static_cast<unsigned>(qe->queries()));
            }
            qe->accept();
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
        const char* platform_str = filter->ime_adapter.platform_name();
        const QMetaObject* meta = item->metaObject();
        QStringList animation_signals;
        if (meta) {
            for (int i = 0; i < meta->methodCount(); ++i) {
                QMetaMethod method = meta->method(i);
                if (method.methodType() != QMetaMethod::Signal) continue;
                const QByteArray sig = method.methodSignature();
                if (sig.contains("visual") || sig.contains("transaction") || sig.contains("explicit_clear")) {
                    animation_signals << QString::fromLatin1(sig);
                }
            }
        }
        QByteArray qpa_platform = qEnvironmentVariable("QT_QPA_PLATFORM").toUtf8();
        QByteArray qt_im_module = qEnvironmentVariable("QT_IM_MODULE").toUtf8();
        QByteArray qt_im_modules = qEnvironmentVariable("QT_IM_MODULES").toUtf8();
        QByteArray qt_plugin_path = qEnvironmentVariable("QT_PLUGIN_PATH").toUtf8();
        qDebug("[sujian] install_event_filter: platform=%s, QT_QPA_PLATFORM=%s, QT_IM_MODULE=%s, "
               "QT_IM_MODULES=%s, QT_PLUGIN_PATH=%s, "
               "ItemAcceptsInputMethod=%d, QInputMethod=%p, hasActiveFocus=%d",
               platform_str,
               qpa_platform.constData(),
               qt_im_module.constData(),
               qt_im_modules.constData(),
               qt_plugin_path.constData(),
               item->flags().testFlag(QQuickItem::ItemAcceptsInputMethod),
               im,
               item->hasActiveFocus());
        qDebug("[sujian] SujianEditorItem metaObject animation signals: class=%s signals=%s",
               meta ? meta->className() : "<null>",
               animation_signals.join(QStringLiteral(",")).toUtf8().constData());
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

pub(crate) fn install_event_filter(item: *mut c_void, rust_item: *mut c_void) {
    cpp!(unsafe [item as "QQuickItem*", rust_item as "void*"] {
        sujian_install_event_filter(item, rust_item);
    });
}

pub(crate) fn focus_item(item: *mut c_void) {
    cpp!(unsafe [item as "QQuickItem*"] {
        sujian_focus_item(item);
    });
}
