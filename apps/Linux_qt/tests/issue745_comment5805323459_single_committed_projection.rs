//! Issue #745 评论 5805323459 — 正文状态收口为单一平台投影。
//!
//! WHITE_BOX 守卫：Qt 端只允许两份正文状态 —— Core `EditorKernel`（业务真相）
//! 和 `CommittedTextMirror`（只读平台投影）。运行时第三份镜像
//! `EditorBuffer`（text/cursor/selection_anchor + undo/redo 栈）必须保持删除状态，
//! 所有正文/光标/选区读取都走 pipeline 投影；退格/前删的编辑边界由 Core
//! `EditorKernel::previous/next_grapheme_boundary` 唯一决定，不允许 Qt 端按
//! Unicode scalar 自己算。
//!
//! 背景：`issue_683_repro` 留下过 `mirror.cursor` 与 `buffer.cursor` 不一致后
//! 删错字符的复现记录；多一份可写镜像就多一次漂移机会。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 判断 `needle` 是否以"独立标识符"形式出现（左右都不是标识符字符）。
/// 用来区分旧 Buffer 的 `undo_stack` 字段和仍然合法的 `clear_undo_stack` 方法名。
fn contains_bare_identifier(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    haystack.match_indices(needle).any(|(start, matched)| {
        let end = start + matched.len();
        let left_ok = start == 0 || !is_ident(bytes[start - 1]);
        let right_ok = end == bytes.len() || !is_ident(bytes[end]);
        left_ok && right_ok
    })
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("src dir must exist") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// 取出某个方法从签名到函数体结束之间的文本。
fn method_body(src: &str, signature: &str) -> String {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("method `{}` must exist", signature));
    let rest = &src[start..];
    let end = rest
        .find("\n    }\n")
        .unwrap_or_else(|| panic!("method `{}` body end not found", signature));
    rest[..end].to_string()
}

/// 测试 1: 第三份镜像 `EditorBuffer` 及其只服务旧 Buffer 的实现必须彻底删除。
#[test]
fn issue745_runtime_third_mirror_editor_buffer_is_gone() {
    assert!(
        !linux_qt_root()
            .join("src/sujian_editor_item/buffer.rs")
            .exists(),
        "buffer.rs 必须删除，Qt 端不再持有第三份可编辑正文镜像"
    );

    let mut files = Vec::new();
    collect_rs(&linux_qt_root().join("src"), &mut files);
    for path in files {
        let src = std::fs::read_to_string(&path).expect("rs file must be readable");
        let rel = path
            .strip_prefix(linux_qt_root())
            .unwrap_or(path.as_path())
            .display()
            .to_string();
        for forbidden in [
            "EditorBuffer",
            "self.buffer",
            "item.buffer",
            "sync_buffer_from_pipeline",
            "push_undo",
            "undo_stack",
            "redo_stack",
        ] {
            assert!(
                !contains_bare_identifier(&src, forbidden),
                "{rel} 仍引用已删除的旧 Buffer 实现 `{forbidden}`"
            );
        }
    }
}

/// 测试 2: `EditorSnapshot` 与可编辑 Buffer 解耦，纯转换函数搬到 text_utils。
#[test]
fn issue745_snapshot_and_pure_converters_are_split_out() {
    let snapshot_src = read_src("src/sujian_editor_item/edit_snapshot.rs");
    assert!(
        snapshot_src.contains("pub struct EditorSnapshot"),
        "EditorSnapshot 必须留在 edit_snapshot.rs"
    );
    for field in ["pub text:", "pub cursor:", "pub selection_anchor:"] {
        assert!(
            snapshot_src.contains(field),
            "动画/事务快照需要 text/cursor/selection_anchor，缺 `{field}`"
        );
    }
    for forbidden in [
        "EditorBuffer",
        "undo_stack",
        "redo_stack",
        "Vec<EditorSnapshot>",
    ] {
        assert!(
            !snapshot_src.contains(forbidden),
            "edit_snapshot.rs 只保留动画/事务需要的快照，不应出现 `{forbidden}`"
        );
    }

    let utils_src = read_src("src/sujian_editor_item/text_utils.rs");
    for func in [
        "pub fn normalize_plain_text",
        "pub fn byte_to_char_index",
        "pub fn clamp_to_char_boundary",
    ] {
        assert!(
            utils_src.contains(func),
            "纯转换函数 `{func}` 必须在 text_utils.rs"
        );
    }
    assert!(
        !utils_src.contains("EditorBuffer"),
        "text_utils.rs 只有纯转换，不绑定 Buffer 类型"
    );

    let mod_src = read_src("src/sujian_editor_item/mod.rs");
    assert!(
        mod_src.contains("pub(crate) mod edit_snapshot;")
            && mod_src.contains("pub(crate) mod text_utils;"),
        "mod.rs 必须导入 edit_snapshot / text_utils 子模块"
    );
    assert!(
        mod_src.contains("use edit_snapshot::EditorSnapshot;"),
        "mod.rs 的 EditorSnapshot 必须来自 edit_snapshot，而不是 Buffer 模块"
    );
    assert!(
        !mod_src.contains("buffer:"),
        "SujianEditorItem 不得再持有 buffer 字段"
    );
}

/// 测试 3: pipeline 暴露只读 committed 投影 API，grapheme 边界委托给 Core kernel。
#[test]
fn issue745_pipeline_exposes_read_only_committed_projection() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    for api in [
        "pub fn committed_text(&self) -> &str",
        "pub fn cursor(&self) -> usize",
        "pub fn selection_anchor(&self) -> usize",
        "pub fn selection_range(&self) -> (usize, usize)",
        "pub fn has_selection(&self) -> bool",
        "pub fn snapshot(&self) -> EditorSnapshot",
    ] {
        assert!(src.contains(api), "pipeline 缺少只读投影 API `{api}`");
    }

    // 投影 API 只能是 mirror 的只读委托，不得自带一份可写正文。
    let committed_body = method_body(&src, "pub fn committed_text(&self) -> &str");
    assert!(
        committed_body.contains("self.mirror.text()"),
        "committed_text 必须直接委托 CommittedTextMirror"
    );

    assert!(
        src.contains("self.kernel.previous_grapheme_boundary(")
            && src.contains("self.kernel.next_grapheme_boundary("),
        "grapheme 边界必须委托 Core EditorKernel，不允许 Qt 端自己按 char 算"
    );
}

/// 测试 4: 退格/前删的编辑边界取自 pipeline/Core kernel 的 grapheme 边界。
#[test]
fn issue745_backspace_and_delete_use_grapheme_boundaries() {
    let src = read_src("src/sujian_editor_item/editing.rs");

    let backward = method_body(&src, "pub(crate) fn delete_backward(&mut self)");
    assert!(
        backward.contains("self.pipeline.previous_grapheme_boundary("),
        "delete_backward 的删除起点必须用 pipeline.previous_grapheme_boundary"
    );
    assert!(
        !backward.contains("prev_char_boundary("),
        "delete_backward 不得再按 Unicode scalar 自己算编辑边界"
    );

    let forward = method_body(&src, "pub(crate) fn delete_forward(&mut self)");
    assert!(
        forward.contains("self.pipeline.next_grapheme_boundary("),
        "delete_forward 的删除终点必须用 pipeline.next_grapheme_boundary"
    );
    assert!(
        !forward.contains("next_char_boundary("),
        "delete_forward 不得再按 Unicode scalar 自己算编辑边界"
    );

    // 两个入口都从同一份投影读状态，不再先同步本地副本。
    for body in [&backward, &forward] {
        assert!(
            body.contains("self.pipeline.cursor()")
                && body.contains("self.pipeline.has_selection()")
                && body.contains("self.pipeline.selection_range()"),
            "删除入口必须直接读 pipeline 投影的 cursor/选区"
        );
    }
}

/// 测试 5: 正文/光标/选区消费者只剩 pipeline 投影。
#[test]
fn issue745_consumers_read_committed_projection() {
    let consumers = [
        "src/sujian_editor_item/properties.rs",
        "src/sujian_editor_item/editing.rs",
        "src/sujian_editor_item/input_host.rs",
        "src/sujian_editor_item/ime_visual.rs",
        "src/sujian_editor_item/layout_ops.rs",
        "src/sujian_editor_item/rendering.rs",
        "src/sujian_editor_item/qquickitem_impl.rs",
        "src/sujian_editor_item/mod.rs",
    ];
    let mut hits = 0usize;
    for rel in consumers {
        let src = read_src(rel);
        let reads_projection = [
            "pipeline.committed_text()",
            "pipeline.cursor()",
            "pipeline.selection_anchor()",
            "pipeline.selection_range()",
            "pipeline.has_selection()",
            "pipeline.selected_text()",
            "pipeline.snapshot()",
        ]
        .iter()
        .any(|api| src.contains(api));
        if reads_projection {
            hits += 1;
        }
        assert!(
            !src.contains("self.buffer") && !src.contains("EditorBuffer"),
            "{rel} 仍在读取已删除的 Qt 本地正文副本"
        );
    }
    assert!(
        hits >= 6,
        "正文消费者必须改读 pipeline committed 投影，实际命中 {hits}/{}",
        consumers.len()
    );

    // old/new 动画快照只在编辑前后各取一次投影，不成长期状态。
    let props = read_src("src/sujian_editor_item/properties.rs");
    assert!(
        props.matches("self.pipeline.snapshot()").count() >= 2,
        "set/reload plain_text 必须在编辑前后各取一次 pipeline.snapshot()"
    );
}
