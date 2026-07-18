//! Linux Qt TextInputAdapter 实现
//!
//! 将 Qt keyPressEvent / inputMethodEvent 转换为 NormalizedTextInputEvent。
//! UTF-16 ↔ UTF-8 转换只在此处做一次，编辑器内部只用 UTF-8 byte offset。

use std::any::Any;

use writer_core::platform_interaction::text_input::{
    NormalizedKey, NormalizedModifiers, NormalizedPreeditAttribute,
    NormalizedTextInputEvent, PlatformRawInputEvent, TextInputAdapter,
};

use super::ime_platform::ImePlatformDetector;
use super::utf16_converter;

/// Linux Qt 原始输入事件
pub struct LinuxQtRawInputEvent {
    pub kind: LinuxQtRawInputKind,
}

/// Linux Qt 原始输入事件类型
pub enum LinuxQtRawInputKind {
    KeyPress {
        key: i32,
        modifiers: i32,
        text: String,
    },
    ImePreedit {
        text: String,
        cursor: i32,
        attributes: Vec<LinuxQtPreeditAttribute>,
    },
    ImeCommit {
        text: String,
    },
    ImeReplacementCommit {
        text: String,
        replace_start: i32,
        replace_length: i32,
    },
    ImeCancel,
}

/// Linux Qt preedit 属性（从 Qt QInputMethodEvent::Attribute 映射）
pub struct LinuxQtPreeditAttribute {
    pub attr_type: i32,
    pub start: i32,
    pub length: i32,
    pub value: Option<String>,
}

impl PlatformRawInputEvent for LinuxQtRawInputEvent {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

const QT_KEY_BACKSPACE: i32 = 0x0100_0003;
const QT_KEY_TAB: i32 = 0x0100_0001;
const QT_KEY_ENTER: i32 = 0x0100_0005;
const QT_KEY_INSERT: i32 = 0x0100_0006;
const QT_KEY_RETURN: i32 = 0x0100_0004;
const QT_KEY_DELETE: i32 = 0x0100_0007;
const QT_KEY_LEFT: i32 = 0x0100_0012;
const QT_KEY_UP: i32 = 0x0100_0013;
const QT_KEY_RIGHT: i32 = 0x0100_0014;
const QT_KEY_DOWN: i32 = 0x0100_0015;
const QT_KEY_HOME: i32 = 0x0100_0010;
const QT_KEY_END: i32 = 0x0100_0011;
const QT_KEY_ESCAPE: i32 = 0x0100_0000;
const QT_KEY_PAGEUP: i32 = 0x0100_0016;
const QT_KEY_PAGEDOWN: i32 = 0x0100_0017;
const QT_CTRL_MODIFIER: i32 = 0x0400_0000;
const QT_SHIFT_MODIFIER: i32 = 0x0200_0000;
const QT_ALT_MODIFIER: i32 = 0x0800_0000;
const QT_META_MODIFIER: i32 = 0x1000_0000;

/// Linux Qt TextInputAdapter 实现
pub struct LinuxQtTextInputAdapter {
    ime_detector: ImePlatformDetector,
}

impl LinuxQtTextInputAdapter {
    pub fn new() -> Self {
        let mut adapter = Self {
            ime_detector: ImePlatformDetector::new(),
        };
        adapter.ime_detector.detect_from_env();
        adapter
    }

    pub fn ime_detector(&self) -> &ImePlatformDetector {
        &self.ime_detector
    }

    pub fn ime_detector_mut(&mut self) -> &mut ImePlatformDetector {
        &mut self.ime_detector
    }

    fn convert_key(key: i32) -> NormalizedKey {
        match key {
            QT_KEY_BACKSPACE => NormalizedKey::Backspace,
            QT_KEY_TAB => NormalizedKey::Tab,
            QT_KEY_RETURN | QT_KEY_ENTER => NormalizedKey::Enter,
            QT_KEY_INSERT => NormalizedKey::Insert,
            QT_KEY_DELETE => NormalizedKey::Delete,
            QT_KEY_LEFT => NormalizedKey::Left,
            QT_KEY_UP => NormalizedKey::Up,
            QT_KEY_RIGHT => NormalizedKey::Right,
            QT_KEY_DOWN => NormalizedKey::Down,
            QT_KEY_HOME => NormalizedKey::Home,
            QT_KEY_END => NormalizedKey::End,
            QT_KEY_ESCAPE => NormalizedKey::Escape,
            QT_KEY_PAGEUP => NormalizedKey::PageUp,
            QT_KEY_PAGEDOWN => NormalizedKey::PageDown,
            _ if key >= 0x20 && key < 0x0100_0000 => NormalizedKey::Char(key as u32),
            _ => NormalizedKey::Unknown(key as u32),
        }
    }

    fn convert_modifiers(modifiers: i32) -> NormalizedModifiers {
        NormalizedModifiers {
            ctrl: modifiers & QT_CTRL_MODIFIER != 0,
            shift: modifiers & QT_SHIFT_MODIFIER != 0,
            alt: modifiers & QT_ALT_MODIFIER != 0,
            meta: modifiers & QT_META_MODIFIER != 0,
        }
    }

    fn convert_preedit_attribute(attr: &LinuxQtPreeditAttribute) -> NormalizedPreeditAttribute {
        match attr.attr_type {
            0 => NormalizedPreeditAttribute::Underline,
            1 => NormalizedPreeditAttribute::Cursor,
            2 => NormalizedPreeditAttribute::TextColor {
                color: attr.value.clone().unwrap_or_default(),
            },
            3 => NormalizedPreeditAttribute::BackgroundColor {
                color: attr.value.clone().unwrap_or_default(),
            },
            4 => NormalizedPreeditAttribute::FontUnderline,
            _ => NormalizedPreeditAttribute::Underline,
        }
    }

    /// 直接从 LinuxQtRawInputKind 转换（无需 dyn trait）
    pub fn normalize_raw(&self, raw: &LinuxQtRawInputKind) -> NormalizedTextInputEvent {
        match raw {
            LinuxQtRawInputKind::KeyPress { key, modifiers, text } => {
                let norm_key = Self::convert_key(*key);
                let norm_mods = Self::convert_modifiers(*modifiers);
                if norm_mods.ctrl || norm_mods.alt || norm_mods.meta {
                    NormalizedTextInputEvent::Shortcut {
                        key: norm_key,
                        modifiers: norm_mods,
                    }
                } else if !text.is_empty() {
                    match norm_key {
                        NormalizedKey::Backspace
                        | NormalizedKey::Delete
                        | NormalizedKey::Left
                        | NormalizedKey::Right
                        | NormalizedKey::Up
                        | NormalizedKey::Down
                        | NormalizedKey::Home
                        | NormalizedKey::End
                        | NormalizedKey::Escape
                        | NormalizedKey::Tab
                        | NormalizedKey::Enter
                        | NormalizedKey::Insert
                        | NormalizedKey::PageUp
                        | NormalizedKey::PageDown => NormalizedTextInputEvent::Shortcut {
                            key: norm_key,
                            modifiers: norm_mods,
                        },
                        _ => NormalizedTextInputEvent::PlainText {
                            text: text.clone(),
                        },
                    }
                } else {
                    NormalizedTextInputEvent::Shortcut {
                        key: norm_key,
                        modifiers: norm_mods,
                    }
                }
            }
            LinuxQtRawInputKind::ImePreedit { text, cursor, attributes } => {
                let norm_attrs: Vec<NormalizedPreeditAttribute> = attributes
                    .iter()
                    .map(Self::convert_preedit_attribute)
                    .collect();
                NormalizedTextInputEvent::PreeditChanged {
                    text: text.clone(),
                    cursor: (*cursor).max(0) as usize,
                    attributes: norm_attrs,
                }
            }
            LinuxQtRawInputKind::ImeCommit { text } => NormalizedTextInputEvent::ImeCommit {
                text: text.clone(),
            },
            LinuxQtRawInputKind::ImeReplacementCommit { text, replace_start, replace_length } => {
                NormalizedTextInputEvent::ImeReplacementCommit {
                    text: text.clone(),
                    replace_start: *replace_start,
                    replace_length: *replace_length,
                }
            }
            LinuxQtRawInputKind::ImeCancel => NormalizedTextInputEvent::ImeCancel,
        }
    }
}

impl Default for LinuxQtTextInputAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TextInputAdapter for LinuxQtTextInputAdapter {
    fn normalize_input_event(
        &self,
        raw: &dyn PlatformRawInputEvent,
    ) -> Option<NormalizedTextInputEvent> {
        let raw = raw.as_any().downcast_ref::<LinuxQtRawInputEvent>()?;
        Some(self.normalize_raw(&raw.kind))
    }

    fn is_ime_composing(&self) -> bool {
        self.ime_detector.is_ime_composing()
    }

    fn utf16_to_utf8_offset(&self, text: &str, utf16_offset: usize) -> usize {
        utf16_converter::utf16_to_utf8_offset(text, utf16_offset)
    }

    fn utf8_to_utf16_offset(&self, text: &str, utf8_offset: usize) -> usize {
        utf16_converter::utf8_to_utf16_offset(text, utf8_offset)
    }
}
