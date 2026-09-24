// ── Qt 文本布局模块 ──
//
// 坐标空间约定：
// - Qt 层（本模块）：QChar index（UTF-16 code unit），与 QTextLayout/QTextLine API 一致
// - Core 层：UTF-8 byte offset
// - 转换入口：`sujian_editor_item` 中的 `utf8_to_utf16` / `utf16_to_utf8`
//   在调用本模块函数前完成坐标转换
//
// 线程安全：`g_editor_layout_buf` 为 thread_local，
// 仅在 GUI 线程中使用，不跨线程共享。
//
// 模块拆分（Issue #748 评论 5805325152 / 5810761209）：
// - types: 纯数据类型
// - qt_cache: QTextLayout/QTextLine generation cache
// - engine: 段落排版核心、几何 helper、坐标转换
// - hit_test: hit test / caret / cursor 定位
// - canonical_snapshot: Canonical document visual snapshot
// - diff: VisualLine diff / affected paragraph ranges
// - test_support: 测试线程 helper (run_on_qt_thread)，仅 test/test-helpers feature 下编译

mod canonical_snapshot;
mod diff;
mod engine;
mod hit_test;
mod qt_cache;
mod test_support;
mod types;

// ── re-export：外部公开类型 ──
pub use canonical_snapshot::{
    assemble_document_visual_snapshot_from_lines, inject_animation_visuals_into_snapshot,
    prepare_affected_paragraphs_visual_snapshot, prepare_animation_visuals_from_layout,
    prepare_document_visual_snapshot, prepare_document_visual_snapshot_scoped,
    CanonicalClusterSnapshot, CanonicalDocumentVisualSnapshot, CanonicalLineSnapshot,
    CanonicalParagraphSnapshot, CursorXMapEntry,
};
pub use diff::{compare_old_new_visual_lines, compute_affected_paragraph_ranges, VisualLineDiff};
pub use engine::{
    byte_offset_to_qchar_offset, cursor_rect_for_line, get_font_ascent, get_font_descent,
    qchar_offset_to_byte_offset, text_baseline_y,
};
pub use hit_test::{
    affinity_for_index_on_line, calculate_cursor_x_for_line, caret_rect, caret_rect_doc,
    cursor_line_and_x, hit_test, index_at_line_x, line_contains_cursor_with_affinity,
};
pub use qt_cache::{
    begin_layout_generation, clear_layout_generation, get_paragraph_layout_cursor_to_x_on_line,
    get_paragraph_layout_x_to_cursor_on_line,
};
#[cfg(any(test, feature = "test-helpers"))]
pub use test_support::run_on_qt_thread;
pub use types::{
    CaretAffinity, CaretRect, CursorLayoutRect, LayoutParams, LayoutSnapshot, PreparedLayoutHandle,
    PromotedLayout, VisualLine,
};

/// 编辑器布局引擎 — 管理 QTextLayout 排版缓存。
///
/// 线程约束：QTextLayout 只能在 GUI 线程使用，EditorLayout 不可跨线程。
/// 缓存策略：snapshot() 在参数/revision 不变时复用缓存，避免重复排版。
/// Issue #658 评论 5620035970 问题 2: 持有 current_generation，
/// invalidate 时 clear 旧 generation，重新排版时分配新 generation。
#[derive(Default)]
pub struct EditorLayout {
    cache: Option<LayoutSnapshot>,
    current_generation: u64,
}

impl EditorLayout {
    pub fn invalidate(&mut self) {
        // Issue #658 评论 5620035970 问题 2: 失效时释放旧 generation 的 layout cache。
        if self.current_generation != 0 {
            clear_layout_generation(self.current_generation);
            self.current_generation = 0;
        }
        self.cache = None;
    }

    pub fn cache(&self) -> Option<&LayoutSnapshot> {
        self.cache.as_ref()
    }

    /// Issue #658 评论 5624570557 问题 1: 获取当前有效的 prepared layout 句柄（如果存在）。
    ///
    /// 用于动画 old 帧从已有 QTextLine 提取视觉资源，不再重新排版。
    /// 返回的句柄包含 generation 和 VisualLine 数组引用，
    /// 调用方可用 `prepare_animation_visuals_from_layout` 按需提取动画资源。
    pub fn current_prepared_layout(&self) -> Option<PreparedLayoutHandle<'_>> {
        self.cache.as_ref().map(|c| PreparedLayoutHandle {
            generation: c.layout_generation,
            lines: &c.lines,
        })
    }

    /// Issue #658 评论 5622829886 问题 1: 把外部已排好的 prepared layout 提升为 current。
    ///
    /// 由 record_visual_transaction 全篇排版 new text 后调用，
    /// 把 new_generation 和 visual_lines 直接设为 current，
    /// 后续 snapshot() 发现 cache 有效（text_revision/text_ptr/text_len 匹配）直接返回，
    /// 不再重新排版同一 new text。
    /// 旧 current_generation（若存在且不同于 promoted.generation）会被 clear。
    /// text_revision / text_ptr / text_len 从当前 buffer.text 和 pipeline.text_revision()
    /// 获取，确保与 snapshot() 的 cache 有效性检查一致。
    /// Issue #658 评论 5624570557 问题 1: 同时释放 old_generation（如果非 0）。
    pub fn promote_prepared_layout(
        &mut self,
        promoted: PromotedLayout,
        text: &str,
        text_revision: u64,
    ) {
        let text_ptr = text.as_ptr() as usize;
        let text_len = text.len();
        // 释放旧 generation（若存在且不同于新 generation）
        if self.current_generation != 0 && self.current_generation != promoted.generation {
            clear_layout_generation(self.current_generation);
        }
        // Issue #658 评论 5624570557 问题 1: 释放 old_generation（old 动画使用的 generation）
        if promoted.old_generation != 0 && promoted.old_generation != promoted.generation {
            clear_layout_generation(promoted.old_generation);
        }
        self.current_generation = promoted.generation;
        self.cache = Some(LayoutSnapshot {
            text_revision,
            text_ptr,
            text_len,
            width: promoted.width,
            font_size: promoted.font_size,
            font_family: promoted.font_family,
            line_spacing: promoted.line_spacing,
            text_indent: promoted.text_indent,
            padding: promoted.padding,
            lines: promoted.visual_lines,
            layout_generation: promoted.generation,
        });
    }

    pub fn snapshot(
        &mut self,
        text: &str,
        params: LayoutParams,
        text_revision: u64,
    ) -> &LayoutSnapshot {
        let text_ptr = text.as_ptr() as usize;
        let text_len = text.len();
        let needs_refresh = match &self.cache {
            Some(c) => {
                c.text_revision != text_revision
                    || c.text_ptr != text_ptr
                    || c.text_len != text_len
                    || (c.width - params.width).abs() > 0.1
                    || (c.font_size - params.font_size).abs() > 0.1
                    || c.font_family != params.font_family
                    || (c.line_spacing - params.line_spacing).abs() > 0.01
                    || (c.text_indent - params.text_indent).abs() > 0.1
                    || (c.padding - params.padding).abs() > 0.1
            }
            None => true,
        };

        if needs_refresh {
            // Issue #658 评论 5620035970 问题 2: 重新排版前释放旧 generation，
            // 分配新 generation，避免静态正文和动画/IME 互相清空 layout cache。
            if self.current_generation != 0 {
                clear_layout_generation(self.current_generation);
            }
            self.current_generation = begin_layout_generation();
            self.cache = None;
        }

        self.cache.get_or_insert_with(|| {
            // Issue #658 评论 5622829886 问题 2: 分离基础排版与动画视觉生成。
            // 静态正文只做基础排版（QTextLayout + VisualLine + cursor map），
            // 不生成 QImage/glyphRuns/cluster（generate_animation_visuals=false）。
            // QSGTextNode 直接消费已排好的 QTextLayout（通过 generation cache），
            // 不需要先把静态正文画进 QImage。同一正文状态的每段只调用一次
            // prepare_paragraph_layout_core，QTextLayout 存入
            // g_layout_generations[self.current_generation]。
            // 真正需要 line images 的动画/IME 路径各自分配独立 generation
            // 调 prepare_document_visual_snapshot 传 generate_animation_visuals=true。
            // Issue #688: 静态布局准备不接收颜色参数；text_color 仅在
            // generate_animation_visuals=true 时用于 QImage 绘制。
            let doc_snapshot = prepare_document_visual_snapshot(
                text,
                text_revision,
                f64::from(params.font_size),
                &params.font_family,
                f64::from(params.line_spacing),
                f64::from(params.padding),
                f64::from(params.text_indent),
                params.width,
                1.0,
                self.current_generation,
                false,
            );
            LayoutSnapshot {
                text_revision,
                text_ptr,
                text_len,
                width: params.width,
                font_size: params.font_size,
                font_family: params.font_family,
                line_spacing: params.line_spacing,
                text_indent: params.text_indent,
                padding: params.padding,
                lines: doc_snapshot.visual_lines,
                layout_generation: self.current_generation,
            }
        })
    }

    pub fn hit_test(
        &self,
        snapshot: &LayoutSnapshot,
        x: f64,
        y: f64,
        scroll_y: f64,
    ) -> (usize, CaretAffinity) {
        hit_test(snapshot, x, y, scroll_y)
    }

    pub fn caret_rect(
        &self,
        snapshot: &LayoutSnapshot,
        cursor_byte: usize,
        affinity: CaretAffinity,
        scroll_y: f64,
        viewport_h: f64,
    ) -> CaretRect {
        caret_rect(snapshot, cursor_byte, affinity, scroll_y, viewport_h)
    }

    /// Issue #722 评论 5748596920 问题1: canonical caret 文档坐标入口。
    ///
    /// 与 `caret_rect` 的区别：返回的 `y` / `baseline_y` 是文档坐标（不减 scroll_y），
    /// `visible` 始终为 true（文档坐标版本不关心视口可见性）。
    /// 供正文事务 `record_visual_transaction` 构造 old/new caret track 使用，
    /// 使 caret track 与 AnimatedSlice/StaticPatch 文档坐标系一致；
    /// scene graph 在渲染时按当前 scroll_y 做 viewport transform。
    pub fn caret_rect_doc(
        &self,
        snapshot: &LayoutSnapshot,
        cursor_byte: usize,
        affinity: CaretAffinity,
    ) -> CaretRect {
        caret_rect_doc(snapshot, cursor_byte, affinity)
    }

    pub fn cursor_line_and_x(
        &self,
        snapshot: &LayoutSnapshot,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> Option<(usize, f64)> {
        cursor_line_and_x(snapshot, cursor, affinity)
    }

    pub fn index_at_line_x(&self, snapshot: &LayoutSnapshot, line: &VisualLine, x: f64) -> usize {
        index_at_line_x(snapshot, line, x)
    }

    pub fn cursor_x_for_line(
        &self,
        snapshot: &LayoutSnapshot,
        line: &VisualLine,
        cursor: usize,
        affinity: CaretAffinity,
    ) -> f64 {
        calculate_cursor_x_for_line(line, cursor, affinity, snapshot)
    }

    /// Issue #748: 用 QFontMetricsF::horizontalAdvance 测量文本宽度（纯 QFont 测量），
    /// 不创建 QTextLayout，符合"正式路径只从 PreparedLayoutHandle/QTextLine 走"的要求。
    /// 供 preedit/IME 文本宽度测量，不进入排版生命周期。
    pub fn text_width(&self, text: &str, font_size: f64, font_family: &str) -> f64 {
        engine::text_width(text, font_size, font_family)
    }

    pub fn affinity_for_index_on_line(&self, line: &VisualLine, index: usize) -> CaretAffinity {
        affinity_for_index_on_line(line, index)
    }
}
