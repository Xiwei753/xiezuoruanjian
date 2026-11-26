//! Issue #1063 复审：打开/重载正文必须逐字节保留原文，不得静默归一化。
//!
//! 归一化（U+2029/CRLF/孤立 CR → `\n`、过滤控制字符）只允许发生在真实
//! 插入/导入命令边界（`editing.rs`）。本模块在 `properties` 内部声明，
//! 直接验证 `set_plain_text_from_qml` / `load_document_text` /
//! `reload_plain_text` 三条装载路径的保真行为与段落起点偏移。

use super::super::SujianEditorItem;
use crate::editor::layout::run_on_qt_thread;
use qmetaobject::QString;
use writer_core::editor::text_document_rules::paragraph_start_offsets;

/// CRLF（2 字节）+ U+2029（3 字节）+ 控制字符的混合原文。
const MIXED_RAW: &str = "a\r\nb\u{2029}c\u{0007}";

#[test]
fn load_document_text_preserves_raw_bytes() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();
        item.load_document_text(QString::from(MIXED_RAW));

        let mirror_text = item.pipeline.mirror().text();
        assert_eq!(
            mirror_text.as_bytes(),
            MIXED_RAW.as_bytes(),
            "load_document_text 必须逐字节保留原文：CRLF 保持 2 字节、U+2029 保持 3 字节、控制字符不丢"
        );
        assert_eq!(
            mirror_text.len(),
            MIXED_RAW.len(),
            "正文 byte 长度必须与输入一致"
        );
        assert_eq!(
            item.pipeline.text_revision(),
            item.pipeline.mirror().revision(),
            "load_document_text 后 text_revision 必须等于 Core mirror revision"
        );
    });
}

#[test]
fn set_plain_text_and_reload_plain_text_preserve_separators_and_offsets() {
    run_on_qt_thread(|| {
        let mut item = SujianEditorItem::default();

        // set_plain_text：CRLF 是两个独立 byte，第二段正文起点 = 3。
        item.set_plain_text(QString::from("a\r\nb"));
        assert_eq!(
            item.pipeline.mirror().text(),
            "a\r\nb",
            "set_plain_text 不得把 CRLF 压成单个 '\\n'"
        );
        assert_eq!(
            paragraph_start_offsets(item.pipeline.mirror().text()),
            vec![0, 3],
            "CRLF 后的第二段起点必须是 3"
        );

        // reload_plain_text：U+2029 保持 3 字节，第二段正文起点 = 4。
        item.reload_plain_text(QString::from("a\u{2029}b"));
        assert_eq!(
            item.pipeline.mirror().text(),
            "a\u{2029}b",
            "reload_plain_text 不得把 U+2029 替换为 '\\n'"
        );
        assert_eq!(
            paragraph_start_offsets(item.pipeline.mirror().text()),
            vec![0, 4],
            "U+2029 后的第二段起点必须是 4"
        );
        assert_eq!(
            item.pipeline.text_revision(),
            item.pipeline.mirror().revision(),
            "reload_plain_text 后 text_revision 必须等于 Core mirror revision"
        );

        // reload_plain_text：换回 CRLF 原文同样逐字节保留。
        item.reload_plain_text(QString::from("a\r\nb"));
        assert_eq!(item.pipeline.mirror().text(), "a\r\nb");
        assert_eq!(
            paragraph_start_offsets(item.pipeline.mirror().text()),
            vec![0, 3]
        );

        // set_plain_text：U+2029 同样逐字节保留，第二段起点 = 4。
        item.set_plain_text(QString::from("a\u{2029}b"));
        assert_eq!(
            item.pipeline.mirror().text(),
            "a\u{2029}b",
            "set_plain_text 不得把 U+2029 替换为 '\\n'"
        );
        assert_eq!(
            paragraph_start_offsets(item.pipeline.mirror().text()),
            vec![0, 4],
            "U+2029 后的第二段起点必须是 4"
        );
    });
}
