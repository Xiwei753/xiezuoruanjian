//! Issue #1062 评论 7187981523 — 开章版本断线的接线守卫。
//!
//! WHITE_BOX 验证策略：QML 无法在本仓库的 Rust 测试里实例化（组件依赖
//! `qml_resources` qrc 与 Rust 注册的上下文属性），因此按仓库既有惯例
//! （见 `issue762_comment5826175490_qml_wiring.rs`）读取源码，确定性断言
//! 这次修复的接线不变量，防止回退：
//!
//! 1. `writing_bridge.rs` 的开章数据带真实 `version`，且来自
//!    `ChapterContentDto.version`，不从章节目录 meta.hash 拼；
//! 2. `chapter_operations.rs` 两条开章入口输出同一份版本；
//! 3. `EditorController.qml` 用显式字段/整数校验版本（revision=0 合法），
//!    开章前备份并清空上一章版本，校验失败回滚且不替换正文；
//! 4. 打开文档不再隐式 `normalizePlainText`，正文原样交给编辑器；
//! 5. `flushActiveEditorBeforeSync` 在 saveGuardActive 时不假装 durable，
//!    而是保留 continuation 等 guard 解除后重走完整保存路径。
//!
//! 行为级回归（冷启动首章、开 A→切 B→B 输入→首存→切回 A）用 PySide6
//! offscreen harness 覆盖，见提交说明；Rust 测试守住接线不变量。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

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

/// 取从 `marker` 开始到下一个 `end_marker` 之间的源码窗口（不含 end）。
fn window(src: &str, marker: &str, end_marker: &str) -> String {
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker `{marker}` must exist"));
    let rest = &src[start..];
    let end = rest
        .find(end_marker)
        .unwrap_or_else(|| panic!("end marker `{end_marker}` must exist after `{marker}`"));
    rest[..end].to_string()
}

#[test]
fn chapter_open_dto_carries_core_version() {
    let bridge = read_src("src/writing_bridge.rs");
    let dto = window(&bridge, "pub struct LinuxChapterOpenData {", "\n}");
    assert!(
        dto.contains("pub version: writer_core::api::types::ChapterVersionDto"),
        "开章 DTO 必须带真实 ChapterVersionDto：{dto}"
    );
    let open = window(&bridge, "pub fn open_chapter(", "\n}\n");
    assert!(
        open.contains("let chapter = api.open_chapter(") || open.contains(".open_chapter("),
        "open_chapter 必须调用 Core：{open}"
    );
    assert!(
        open.contains("version: chapter.version") || open.contains("version: content.version"),
        "version 必须来自 Core 的 ChapterContentDto：{open}"
    );
    assert!(
        !open.contains("meta.hash") && !open.contains(".hash"),
        "不得用章节目录 meta.hash 代替版本：{open}"
    );

    let operations = read_src("src/backend/chapter_operations.rs");
    let json_open = window(&operations, "fn open_chapter_json(", "fn open_chapter(");
    assert!(
        json_open.contains("\"version\": content.version"),
        "open_chapter_json 必须输出 version：{json_open}"
    );
}

#[test]
fn qml_requires_complete_version_before_loading_document() {
    let qml = read_src("qml/EditorController.qml");

    let validate = window(
        &qml,
        "function isCompleteChapterVersion(",
        "function applyCommittedChapterVersion",
    );
    assert!(
        validate.contains("typeof version.revision !== \"number\"")
            && validate.contains("Number.isInteger(version.revision)")
            && validate.contains("version.revision < 0"),
        "版本校验必须显式要求非负整数 revision（0 合法）：{validate}"
    );
    assert!(
        validate.contains("typeof version.objectGeneration !== \"string\"")
            && validate.contains("typeof version.contentSha256 !== \"string\""),
        "版本校验必须显式检查 generation/sha 字段类型：{validate}"
    );

    let apply = window(
        &qml,
        "function applyCommittedChapterVersion(",
        "function committedChapterVersionMatches",
    );
    assert!(
        apply.contains("if (!isCompleteChapterVersion(version)) return false;"),
        "不完整版本不得写入 committedChapter*：{apply}"
    );
    assert!(
        apply.contains("committedChapterGeneration = version.objectGeneration;"),
        "完整版本才能安装三元组：{apply}"
    );

    let open = window(
        &qml,
        "function openChapterContentNow(",
        "// Issue #754 评论 5815901258: 清空当前章节编辑器状态。",
    );
    let validate_pos = open
        .find("isCompleteChapterVersion(loadedData.version)")
        .expect("开章必须校验 loadedData.version");
    let load_pos = open
        .find("load_document_text(")
        .expect("开章必须 load_document_text");
    assert!(
        validate_pos < load_pos,
        "版本校验必须在替换正文之前（版本断线 P0）：{open}"
    );
    assert!(
        open.contains("var previousVersion = {")
            && open.contains("chapterVersionInstalled = false;"),
        "开章前必须备份上一章版本并清空 installed：{open}"
    );
    assert!(
        open.contains("finishChapterOpenFailure(pId, vId, cId"),
        "失败必须走统一回滚收口：{open}"
    );
    assert!(
        !open.contains("normalizePlainText("),
        "打开文档不得隐式归一化正文：{open}"
    );
    assert!(
        open.contains("String(result.data.content || \"\")"),
        "打开文档必须使用 Core 返回的原文：{open}"
    );

    let mark_loaded = window(&qml, "function markEditorDocumentLoaded(", "// Issue #1051");
    assert!(
        !mark_loaded.contains("if (!chapterVersionInstalled)"),
        "版本校验属于开章入口；markEditorDocumentLoaded 不能因未安装版本跳过会话重置：{mark_loaded}"
    );

    let flush = window(
        &qml,
        "function flushActiveEditorBeforeSync(",
        "// Issue #1052：同步前的落盘同样只消费同一时点的完整 Core 快照。",
    );
    let guard_start = flush
        .find("if (saveGuardActive()) {")
        .expect("flush 必须显式处理 saveGuardActive");
    let guard = &flush[guard_start..(guard_start + 1400).min(flush.len())];
    assert!(
        guard.contains("pendingSyncFlushContinuation")
            && guard.contains("syncFlushGuardTimer.running = true"),
        "guard 活跃时必须保留 continuation 并等待解除：{guard}"
    );
    assert!(
        !guard.contains("if (finished) finished(true);"),
        "guard 活跃时不得向同步谎报成功：{guard}"
    );
}
