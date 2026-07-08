//! TextInputAdapter — 归一化输入事件
//!
//! 平台 UTF-16/UTF-8/code point/char index 转换只能在 adapter 做一次，
//! 编辑器内部只用统一索引（UTF-8 byte offset）。

use serde::{Deserialize, Serialize};

/// 归一化输入事件 — 平台适配层输出，编辑器消费
///
/// 所有索引均为 UTF-8 byte offset，平台适配层负责一次性转换。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum NormalizedTextInputEvent {
    /// 普通文本插入（键盘直接输入、符号）
    PlainText {
        text: String,
    },
    /// 快捷键
    Shortcut {
        key: NormalizedKey,
        modifiers: NormalizedModifiers,
    },
    /// Preedit 文本变化（IME 组合输入）
    PreeditChanged {
        text: String,
        cursor: usize,
        attributes: Vec<NormalizedPreeditAttribute>,
    },
    /// IME commit 上屏
    ImeCommit {
        text: String,
    },
    /// IME commit 带替换语义（fcitx5 拼音修正等）
    ImeReplacementCommit {
        text: String,
        replace_start: i32,
        replace_length: i32,
    },
    /// IME 取消
    ImeCancel,
}

/// 归一化按键 — 跨平台统一键码
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NormalizedKey {
    Backspace,
    Tab,
    Enter,
    Insert,
    Delete,
    Left,
    Up,
    Right,
    Down,
    Home,
    End,
    Escape,
    PageUp,
    PageDown,
    Char(u32),
    Unknown(u32),
}

/// 归一化修饰键
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Default for NormalizedModifiers {
    fn default() -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }
}

/// 归一化 preedit 属性
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NormalizedPreeditAttribute {
    Underline,
    TextColor { color: String },
    BackgroundColor { color: String },
    FontUnderline,
    Cursor,
}

/// TextInputAdapter trait — 平台适配层实现
///
/// 平台端实现此 trait，将平台特定的输入事件转换为 NormalizedTextInputEvent。
/// 编辑器只消费 NormalizedTextInputEvent，不关心平台细节。
pub trait TextInputAdapter {
    /// 将平台原始输入事件转换为归一化事件
    fn normalize_input_event(&self, raw: &dyn PlatformRawInputEvent) -> Option<NormalizedTextInputEvent>;

    /// 当前是否正在 IME composing
    fn is_ime_composing(&self) -> bool;

    /// 是否可以接受纯文本按键（非 composing 状态时可以）
    fn can_accept_plain_text_key(&self) -> bool {
        !self.is_ime_composing()
    }

    /// UTF-16 offset → UTF-8 byte offset 转换（平台特定）
    fn utf16_to_utf8_offset(&self, text: &str, utf16_offset: usize) -> usize;

    /// UTF-8 byte offset → UTF-16 offset 转换（平台特定）
    fn utf8_to_utf16_offset(&self, text: &str, utf8_offset: usize) -> usize;
}

/// 平台原始输入事件 — 各平台自行定义
///
/// 这是个标记 trait，各平台在自己的适配层中定义具体的原始事件类型，
/// 然后实现 TextInputAdapter::normalize_input_event 来转换。
/// Core 不定义平台特定事件，避免引入平台依赖。
pub trait PlatformRawInputEvent: Any {
    /// 提供 Any 接口用于 downcast
    fn as_any(&self) -> &dyn Any;
}

use std::any::Any;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_event_serializes_camel_case() {
        let event = NormalizedTextInputEvent::PlainText {
            text: "你好".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"plainText\""));
        assert!(json.contains("\"text\":\"你好\""));
    }

    #[test]
    fn normalized_modifiers_default() {
        let mods = NormalizedModifiers::default();
        assert!(!mods.ctrl);
        assert!(!mods.shift);
        assert!(!mods.alt);
        assert!(!mods.meta);
    }

    #[test]
    fn ime_replacement_commit_roundtrip() {
        let event = NormalizedTextInputEvent::ImeReplacementCommit {
            text: "修正".to_string(),
            replace_start: -2,
            replace_length: 2,
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: NormalizedTextInputEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn preedit_changed_with_attributes() {
        let event = NormalizedTextInputEvent::PreeditChanged {
            text: "拼".to_string(),
            cursor: 3,
            attributes: vec![
                NormalizedPreeditAttribute::Underline,
                NormalizedPreeditAttribute::Cursor,
            ],
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: NormalizedTextInputEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
    }
}
