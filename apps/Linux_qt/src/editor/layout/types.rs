// ── Qt 文本布局模块：纯数据类型 ──
//
// 坐标空间约定：
// - Qt 层（本模块）：QChar index（UTF-16 code unit），与 QTextLayout/QTextLine API 一致
// - Core 层：UTF-8 byte offset
// - 转换入口：`sujian_editor_item` 中的 `utf8_to_utf16` / `utf16_to_utf8`
//   在调用本模块函数前完成坐标转换

use std::sync::Arc;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretAffinity {
    Upstream,
    Downstream,
}

// ── 稳定段落身份 ──

/// 稳定段落身份 — 不随 byte offset 变化的段落唯一标识。
///
/// Issue #901 第十八轮问题4: 从 `paragraph_layout_store` 移到 `types`，
/// 使 `VisualLine` 可直接持有 `ParagraphId` 而不产生模块循环依赖。
///
/// 每次创建新段落（换行拆段、粘贴新段、加载文档）时分配一个新的 `ParagraphId`。
/// 段落内容编辑不改变 `ParagraphId`。
/// 删回车合段时，被合并的段落的 `ParagraphId` 失效，保留的段落保持原 Id。
///
/// **不能**以随编辑移动的 byte offset 当稳定对象 identity ——
/// 在段前插入文本会让所有后续段落的 byte offset 移动，但它们的 `ParagraphId` 不变。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParagraphId(u64);

impl ParagraphId {
    /// 占位 Id，表示无效/未分配。
    pub const INVALID: ParagraphId = ParagraphId(0);

    /// 返回内部数值，仅供测试/调试使用。
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Issue #901 第十八轮问题4: 从内部数值构造 `ParagraphId`。
    /// 仅供 `paragraph_layout_store` 分配新 Id 使用，外部不应调用。
    pub(crate) fn from_u64(value: u64) -> ParagraphId {
        ParagraphId(value)
    }
}

/// 视觉行 — 排版后的单行文本，同时持有 UTF-8 byte offset 和 QChar (UTF-16) offset。
///
/// 坐标空间：x/y/width/height 为文档坐标系（不含 scroll offset）。
/// byte_start/byte_end 为半开区间 [byte_start, byte_end)（UTF-8 byte offset，文档级）。
/// qchar_start/qchar_end 为半开区间 [qchar_start, qchar_end)（QChar index，文档级）。
///
/// 段落相关字段（para_text, para_start, para_qchar_start/end, qtextline_idx,
/// line_wrap_width, line_indent_x, para_indent, x_end_trailing）用于
/// 重新调用 QTextLayout API 做精确光标定位和 hit test。
#[derive(Clone, Debug, PartialEq)]
pub struct VisualLine {
    pub id: usize,
    /// Byte offset of the line start in the full document text (UTF-8).
    pub byte_start: usize,
    /// Byte offset of the line end in the full document text (UTF-8).
    pub byte_end: usize,
    /// QChar (UTF-16 code unit) offset of the line start in the full document text.
    pub qchar_start: usize,
    /// QChar (UTF-16 code unit) offset of the line end in the full document text.
    pub qchar_end: usize,
    pub hard_break: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub para_text: String,
    pub para_start: usize,
    pub qtextline_idx: i32,
    pub para_qchar_start: usize,
    pub para_qchar_end: usize,
    pub line_wrap_width: f64,
    pub line_indent_x: f64,
    pub para_indent: f64,
    pub x_end_trailing: f64,
    pub qt_ascent: f64,
    pub qt_descent: f64,
    /// Issue #901 第二十轮阻塞3: Qt 物理缓存槽号 — 传给 C++ qt_cache /
    /// scene_graph_renderer / prepare.rs 的 `get_paragraph_layout(gen, slot)`。
    /// 这是 QTextLayout 在 `g_layout_generations[gen]` 中的物理数组下标，
    /// 由 `editor_prepare_paragraph_visual_snapshot` 在排版时写入。
    /// 插段后复用后缀 QTextLayout 时，后缀行的 `cache_slot` 保持原物理槽号不变，
    /// 不随文档次序变化。
    pub cache_slot: i32,
    /// Issue #901 第二十轮阻塞1+3: 当前文档次序（logical slot）— 段落在文档中的
    /// 顺序索引（含空段落，从 0 递增）。用于查 `paragraph_offset_index` /
    /// `height_index` 的前缀和，以及 `paragraph_id_to_line_range` 行范围索引。
    /// 增量编辑拆段/合段后，后缀段的 logical_slot 会变化（新段插入后后缀段序号 +1），
    /// 但 `cache_slot`（Qt 物理槽）和 `paragraph_id`（稳定身份）不变。
    /// `materialize_line` 用此字段查前缀索引得到段首绝对坐标。
    pub logical_slot: i32,
    /// Issue #901 第十八轮问题4: 稳定段落身份，不随编辑移动的 byte offset 变化。
    /// `materialize_line` 优先用此字段通过 `paragraph_id_to_slot` 映射解析当前 slot，
    /// 不再依赖需要遍历后缀行更新的 `cache_slot`。增量编辑拆段/合段后，
    /// 后缀行的 `cache_slot` 可能过时，但 `paragraph_id` 不变，
    /// `materialize_line` 用 `paragraph_id` 查 `paragraph_id_to_slot` 得到新 slot。
    /// `ParagraphId::INVALID` 时回退到 `cache_slot`（兼容旧路径/fallback）。
    pub paragraph_id: ParagraphId,
}

/// 光标矩形 — 文档坐标系（不含 scroll offset）。
/// visible=false 表示光标在可视区域外，平台端不应绘制。
/// baseline_y 是文字基线 Y 坐标（文档坐标系），从 QTextLine 的真实 ascent/descent
/// 计算，不使用 `top + h * 0.8` 估算。
#[derive(Clone, Debug, PartialEq)]
pub struct CaretRect {
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub visual_line_id: usize,
    pub visible: bool,
    pub baseline_y: f64,
}

pub type CursorLayoutRect = CaretRect;

#[derive(Clone, Debug)]
pub struct LayoutParams {
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
}

/// 布局快照 — 某次排版结果的完整快照，与特定 text_revision 绑定。
/// text_ptr/text_len 用于快速判断文本缓冲区是否变更（指针+长度双重校验）。
/// 缓存失效条件：revision、指针、长度、宽度、字号、字体、行距、缩进或内边距任一变化。
#[derive(Clone, Debug)]
pub struct LayoutSnapshot {
    pub text_revision: u64,
    pub text_ptr: usize,
    pub text_len: usize,
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
    /// Issue #901 第九轮阻塞5: 用 `Arc<Vec<VisualLine>>` 共享视觉行数据，
    /// 避免每次编辑克隆整章 VisualLine（含字符串数据）。
    /// `LayoutSnapshot` 和 `CanonicalDocumentVisualSnapshot.visual_lines`
    /// 通过 `Arc::clone` 共享同一份视觉行数据，编辑时 `Arc::make_mut`
    /// 原地修改（refcount==1）或 COW（refcount>1）。
    ///
    /// Issue #901 第十四轮问题1: `lines` 存储段内相对坐标（y/byte/qchar 相对于段首）。
    /// 绝对坐标由消费方通过 `paragraph_offset_index` + `height_index` 按需计算。
    pub lines: Arc<Vec<VisualLine>>,
    /// Issue #901 第十四轮问题1: 段落前缀偏移索引 — 段首 byte/qchar 坐标权威来源。
    /// 消费方通过 `paragraph_offset_index.paragraph_byte_start(cache_slot) + line.byte_start`
    /// 计算文档级绝对 byte offset。
    ///
    /// Issue #901 第二十一轮问题5: 用 `Arc` 共享，避免 clone O(段落数)。
    pub paragraph_offset_index: std::sync::Arc<super::paragraph_height_index::ParagraphOffsetIndex>,
    /// Issue #901 第十四轮问题1: 段落高度索引 — 段首 y 坐标权威来源。
    /// 消费方通过 `padding + height_index.paragraph_top_y(cache_slot) + line.y`
    /// 计算文档级绝对 y 坐标。
    /// Issue #901 第二十九轮复核阻塞4: 用 Arc 共享，避免每笔 clone O(段落数)。
    pub height_index: std::sync::Arc<super::paragraph_height_index::ParagraphHeightIndex>,
    /// Issue #658 评论 5620035970 问题 2: 布局 generation — 标识本 snapshot
    /// 对应的 g_layout_generations 中的代。渲染时用 (generation, cache_slot) 查找 layout。
    pub layout_generation: u64,
    /// Issue #901 第十九轮问题1: ParagraphId → 当前段落 slot 映射。
    /// materialize_line 用此映射解析 slot，不依赖需要遍历后缀行更新的 cache_slot。
    /// Issue #901 第十九轮问题5: Arc 共享，避免 clone O(段落数)。
    /// Issue #901 第二十一轮问题4: Arc::clone 共享持久维护的映射，O(1)。
    pub paragraph_id_to_slot: std::sync::Arc<std::collections::HashMap<ParagraphId, usize>>,
    /// Issue #901 第二十一轮问题2: 段落行数前缀索引（Fenwick 树）。
    ///
    /// 替代旧的 `paragraph_id_to_line_range: Arc<HashMap<ParagraphId, (usize, usize)>>`。
    /// 旧 HashMap 以全局 visual_lines 数组下标为绝对值，却只对改动段更新，
    /// 换行造成行数变化时全部后缀段范围仍指向旧下标。
    ///
    /// 改用 Fenwick 树维护每段 `visual_line_count` 的前缀和：
    /// - 段落 `i` 的行范围 `[paragraph_line_start(i), paragraph_line_end(i))`。
    /// - 前面段行数变化时，后缀段的行范围由前缀和自动平移，
    ///   不需要逐段更新绝对行下标。
    /// - 查视口时 O(log n) 前缀和查询，不需要反向扫描整个 HashMap。
    /// Issue #901 第二十二轮问题4: Arc 共享行数前缀索引，O(1) clone。
    pub paragraph_line_count_index:
        std::sync::Arc<super::paragraph_height_index::ParagraphLineCountIndex>,
}

#[cfg(test)]
impl LayoutSnapshot {
    /// Issue #826: 单元测试里构造一个不带任何行的空布局快照。
    ///
    /// 遮罩前沿 / Reflow 的测试只关心 cluster 几何，不需要真实排版结果。
    pub(crate) fn empty_for_tests() -> Self {
        Self {
            text_revision: 0,
            text_ptr: 0,
            text_len: 0,
            width: 600.0,
            font_size: 16.0,
            font_family: String::new(),
            line_spacing: 0.0,
            text_indent: 0.0,
            padding: 0.0,
            lines: Arc::new(Vec::new()),
            paragraph_offset_index: std::sync::Arc::new(super::paragraph_height_index::ParagraphOffsetIndex::new()),
            height_index: std::sync::Arc::new(super::paragraph_height_index::ParagraphHeightIndex::new()),
            layout_generation: 0,
            paragraph_id_to_slot: std::sync::Arc::new(std::collections::HashMap::new()),
            paragraph_line_count_index: std::sync::Arc::new(
                super::paragraph_height_index::ParagraphLineCountIndex::new(),
            ),
        }
    }
}

impl LayoutSnapshot {
    /// Issue #901 第十四轮问题1: 将段内相对坐标物化为文档级绝对坐标。
    ///
    /// `lines` 存储段内相对坐标（y/byte/qchar 相对于段首）。
    /// 本方法通过 `paragraph_offset_index` + `height_index` 计算段首绝对坐标，
    /// 加上段内相对偏移得到文档级绝对坐标。
    /// Issue #901 第十五轮问题5a: `global_id` 参数保证行 ID 全局唯一。
    /// 物化时用 visual_lines 中的索引作为全局 id，不再依赖段内 ordinal。
    fn materialize_line(&self, line: &VisualLine, global_id: usize) -> VisualLine {
        // Issue #901 第二十轮阻塞1+3: 用 logical_slot 查前缀索引得到段首绝对坐标。
        // logical_slot 是当前文档次序，与 paragraph_offset_index / height_index 的
        // 索引一致。cache_slot 保持原值（Qt 物理槽），传给 C++ qt_cache。
        // 优先用 paragraph_id_to_slot 映射解析 logical_slot（插段后后缀行 logical_slot
        // 可能过时，但 paragraph_id 不变，通过映射查到新 logical_slot）。
        let logical_slot = if line.paragraph_id != ParagraphId::INVALID {
            self.paragraph_id_to_slot
                .get(&line.paragraph_id)
                .copied()
                .unwrap_or(line.logical_slot as usize)
        } else {
            line.logical_slot as usize
        };
        let para_byte_start = self.paragraph_offset_index.paragraph_byte_start(logical_slot);
        let para_qchar_start = self.paragraph_offset_index.paragraph_qchar_start(logical_slot);
        let para_top_y = f64::from(self.padding) + self.height_index.paragraph_top_y(logical_slot);
        VisualLine {
            byte_start: line.byte_start + para_byte_start,
            byte_end: line.byte_end + para_byte_start,
            qchar_start: line.qchar_start + para_qchar_start,
            qchar_end: line.qchar_end + para_qchar_start,
            y: line.y + para_top_y,
            // para_start 恢复为段首绝对 byte offset，兼容 index_at_line_x 等消费方
            para_start: para_byte_start,
            // Issue #901 第十五轮问题5a: 用全局索引作为 id，保证全局唯一。
            id: global_id,
            hard_break: line.hard_break,
            x: line.x,
            width: line.width,
            height: line.height,
            para_text: line.para_text.clone(),
            qtextline_idx: line.qtextline_idx,
            para_qchar_start: line.para_qchar_start,
            para_qchar_end: line.para_qchar_end,
            line_wrap_width: line.line_wrap_width,
            line_indent_x: line.line_indent_x,
            para_indent: line.para_indent,
            x_end_trailing: line.x_end_trailing,
            qt_ascent: line.qt_ascent,
            qt_descent: line.qt_descent,
            // Issue #901 第二十轮阻塞3: cache_slot 保持原值（Qt 物理槽），
            // 传给 C++ qt_cache / scene_graph_renderer / prepare.rs。
            cache_slot: line.cache_slot,
            // Issue #901 第二十轮阻塞1+3: logical_slot 用解析后的当前文档次序，
            // 供消费方查前缀索引和行范围索引。
            logical_slot: logical_slot as i32,
            // Issue #901 第十八轮问题4: 保留原行稳定段落身份。
            paragraph_id: line.paragraph_id,
        }
    }

    /// Issue #901 第十四轮问题1: 获取单行绝对坐标。
    ///
    /// `lines` 存储段内相对坐标，本方法物化为文档级绝对坐标后返回。
    /// Issue #901 第十五轮问题5a: 行 id 用 lines 中的索引（全局唯一）。
    pub fn resolve_line(&self, idx: usize) -> VisualLine {
        match self.lines.get(idx) {
            Some(vl) => self.materialize_line(vl, idx),
            None => return VisualLine {
                id: 0, byte_start: 0, byte_end: 0, qchar_start: 0, qchar_end: 0,
                hard_break: true, x: 0.0, y: 0.0, width: 0.0,
                height: self.font_size as f64 * self.line_spacing as f64,
                para_text: String::new(), para_start: 0, qtextline_idx: 0,
                para_qchar_start: 0, para_qchar_end: 0, line_wrap_width: 0.0,
                line_indent_x: 0.0, para_indent: 0.0, x_end_trailing: 0.0,
                qt_ascent: 0.0, qt_descent: 0.0, cache_slot: 0, logical_slot: 0,
                paragraph_id: ParagraphId::INVALID,
            },
        }
    }

    /// Issue #901 第十四轮问题1: 空操作 — 无 SuffixTransform，物化在 resolved_lines 中完成。
    pub fn materialize_suffix_in_place(&mut self) {
        // 物化在 resolved_lines / resolve_line 中按需完成。
    }

    /// Issue #901 第十四轮问题1: 返回 lines 的物化绝对坐标 Cow 视图。
    ///
    /// `lines` 存储段内相对坐标，本方法通过 `paragraph_offset_index` + `height_index`
    /// 物化为文档级绝对坐标。消费方（hit_test、caret_rect 等）直接使用返回的绝对坐标。
    /// Issue #901 第十五轮问题5a: 行 id 用 lines 中的索引（全局唯一）。
    pub fn resolved_lines(&self) -> Cow<'_, [VisualLine]> {
        if self.lines.is_empty() {
            return Cow::Borrowed(self.lines.as_slice());
        }
        let materialized: Vec<VisualLine> = self
            .lines
            .iter()
            .enumerate()
            .map(|(idx, line)| self.materialize_line(line, idx))
            .collect();
        Cow::Owned(materialized)
    }

    /// Issue #901 第十五轮问题5b: 物化 `[start, end)` 范围内的行为绝对坐标。
    ///
    /// 与 `resolved_lines` 的区别：只物化指定范围的行，不物化整章，
    /// 避免每帧 O(整章) 物化。`start`/`end` 是 `lines` 中的索引（半开区间）。
    /// 越界索引被 clamp 到 `lines.len()`，`start >= end` 返回空 Vec。
    pub fn resolve_line_range(&self, start: usize, end: usize) -> Vec<VisualLine> {
        if start >= end || start >= self.lines.len() {
            return Vec::new();
        }
        let end = end.min(self.lines.len());
        (start..end)
            .map(|idx| self.materialize_line(&self.lines[idx], idx))
            .collect()
    }

    /// Issue #901 第十五轮问题5b: 物化视口内可见行的绝对坐标。
    ///
    /// 用 `ParagraphHeightIndex` 二分找到可见段范围 `[first_para, last_para_excl)`，
    /// 再用 `cache_slot` 在未物化的 `lines` 上二分查找定位行索引范围，
    /// 只物化可见段内的行（加 `viewport_h.max(100.0)` 边距）。
    ///
    /// 与 `resolved_lines` 的区别：只物化视口附近行，不物化整章。
    /// 返回的行可能包含少量视口外的边缘行（首尾段落的边缘行），
    /// 消费方需自行跳过视口外的行（与原 `resolved_lines` 消费方一致）。
    ///
    /// 无 `height_index` 时退化为物化所有行（与 `resolved_lines` 行为一致）。
    pub fn resolve_viewport_lines(&self, scroll_y: f64, viewport_h: f64) -> Vec<VisualLine> {
        if self.lines.is_empty() {
            return Vec::new();
        }
        let margin = viewport_h.max(100.0);
        let visible_top = scroll_y - margin;
        let visible_bottom = scroll_y + viewport_h + margin;

        if !self.height_index.is_empty() {
            let (first_para, last_para_excl) =
                self.height_index.paragraphs_in_y_range(visible_top, visible_bottom);
            if first_para < last_para_excl {
                // Issue #901 第二十轮阻塞1: 用 paragraph_id_to_line_range 查行范围，
                // 替换 raw partition_point(cache_slot)。
                // 插段后复用后缀行的 cache_slot 没有更新，二分查找找不到正确的行。
                // paragraph_id_to_line_range 按 visual_lines 实际行顺序构建，
                // 通过 ParagraphId → (start, end) 直接定位行范围。
                let (start_idx, end_idx) =
                    compute_line_range_from_logical_slots(self, first_para, last_para_excl);
                return self.resolve_line_range(start_idx, end_idx);
            }
            return Vec::new();
        }

        // 无 height_index：退化为物化所有行。
        self.resolved_lines().into_owned()
    }

    /// Issue #901 第十五轮问题5b: 物化指定段落的所有行为绝对坐标。
    ///
    /// 用 `cache_slot` 在未物化的 `lines` 上二分查找定位段落行范围，
    /// 只物化该段落的行。`para_slot` 是段落在 `height_index` 中的索引
    ///（= `VisualLine.cache_slot`）。
    pub fn resolve_paragraph_lines(&self, para_slot: usize) -> Vec<VisualLine> {
        if self.lines.is_empty() {
            return Vec::new();
        }
        // Issue #901 第二十轮阻塞1: 用 paragraph_id_to_line_range 查行范围，
        // 替换 raw partition_point(cache_slot)。
        // para_slot 是 logical_slot（文档次序），先通过 paragraph_id_to_slot
        // 反查 ParagraphId，再从 paragraph_id_to_line_range 查行范围。
        let (start_idx, end_idx) = compute_line_range_from_logical_slots(self, para_slot, para_slot + 1);
        self.resolve_line_range(start_idx, end_idx)
    }

    /// Issue #901 第二十轮阻塞1: 返回 `[first_logical_slot, last_logical_slot_excl)`
    /// 段落范围对应的 `lines` 行索引范围 `[start_idx, end_idx)`。
    ///
    /// 优先用 `paragraph_id_to_line_range` 索引定位行范围，避免在插段/合段后
    /// 复用后缀行的 raw `logical_slot` 上做 `partition_point` 二分查找找错。
    /// 若索引为空（旧路径/fallback），退化为 `partition_point(logical_slot)`。
    ///
    /// 调用方拿到 `(start_idx, end_idx)` 后可用 `resolve_line_range` 逐行物化。
    pub fn paragraph_line_range(
        &self,
        first_logical_slot: usize,
        last_logical_slot_excl: usize,
    ) -> (usize, usize) {
        compute_line_range_from_logical_slots(self, first_logical_slot, last_logical_slot_excl)
    }
}

/// Issue #901 第二十一轮问题2: 通过 logical_slot 范围计算 visual_lines 行范围。
///
/// `first_logical_slot..last_logical_slot_excl` 是文档次序段落范围。
/// 用 `paragraph_line_count_index`（Fenwick 树）的前缀和 O(log n) 查询：
/// - `start_idx = paragraph_line_count_index.paragraph_line_start(first_logical_slot)`
/// - `end_idx = paragraph_line_count_index.paragraph_line_start(last_logical_slot_excl)`
///
/// 前面段行数变化时，后缀段的行范围由前缀和自动平移，不需要逐段更新绝对行下标，
/// 也不需要反向扫描整个 `paragraph_id_to_slot` HashMap 找 slot 在区间内的 ID。
/// 若索引为空（旧路径/fallback），退化为 `partition_point(logical_slot)`。
fn compute_line_range_from_logical_slots(
    snapshot: &LayoutSnapshot,
    first_logical_slot: usize,
    last_logical_slot_excl: usize,
) -> (usize, usize) {
    // Issue #901 第二十一轮问题2: 优先用 paragraph_line_count_index 前缀和。
    if !snapshot.paragraph_line_count_index.is_empty() {
        let start_idx = snapshot
            .paragraph_line_count_index
            .paragraph_line_start(first_logical_slot);
        let end_idx = snapshot
            .paragraph_line_count_index
            .paragraph_line_start(last_logical_slot_excl);
        return (start_idx, end_idx);
    }

    // Fallback: 用 logical_slot 二分查找。
    // logical_slot 按文档顺序递增，同段落行连续。
    let start_idx = snapshot
        .lines
        .partition_point(|l| l.logical_slot < first_logical_slot as i32);
    let end_idx = snapshot
        .lines
        .partition_point(|l| l.logical_slot < last_logical_slot_excl as i32);
    (start_idx, end_idx)
}

/// Issue #658 评论 5622829886 问题 1: 已排好的 prepared layout 提升为 EditorLayout current。
///
/// 由 record_visual_transaction 全篇排版 new text 后构造，
/// 交给 EditorLayout::promote_prepared_layout 提升为 current，
/// 后续 EditorLayout::snapshot 发现 cache 有效直接返回，不再重新排版。
/// generation 对应的 QTextLayout 已存入 g_layout_generations[generation]，
/// 由 EditorLayout 生命周期管理（invalidate/snapshot 失效时 clear）。
/// text_revision / text_ptr / text_len 在提升时从当前 buffer.text 和
/// pipeline.text_revision() 获取，确保与 EditorLayout::snapshot 的 cache
/// 有效性检查一致。
#[derive(Clone)]
pub struct PromotedLayout {
    pub generation: u64,
    /// Issue #901 第九轮阻塞5: 用 Arc 共享视觉行数据，避免 clone Vec<VisualLine>。
    pub visual_lines: Arc<Vec<VisualLine>>,
    /// Issue #901 第十八轮问题1+2: 完整 canonical snapshot，包含所有空段、
    /// clusters、cursor_x_map、image、index map 等。promotion 直接转交，
    /// 不再从 visual_lines 重建（重建会丢空段和已绘制字形数据）。
    pub canonical_snapshot: super::CanonicalDocumentVisualSnapshot,
    pub width: f64,
    pub font_size: f32,
    pub font_family: String,
    pub line_spacing: f32,
    pub text_indent: f32,
    pub padding: f32,
    /// Issue #658 评论 5624570557 问题 1: 旧的 generation，在 promote 完成后释放。
    /// old 动画纹理提取完成后，等 new prepared layout 真正成为 current，再释放旧 generation。
    pub old_generation: u64,
}

/// Issue #901 第二十九轮复核阻塞3: 可查询排版句柄 — 统一 Qt 槽分配和引用计数所有权模型。
///
/// 封装 `(generation, physical_slot, epoch)` 三元组，提供引用计数操作和 epoch 校验查询。
/// Store entry、VisualLine 关联的可查询排版句柄共享此类型。
///
/// # 所有权模型
///
/// - `acquire_physical_slot_for_generation(gen)` 分配物理槽（refcount=0, epoch 已设）。
/// - `install_layout_slot_gen(gen, slot, layout)` 在已分配的槽上安装 layout（refcount=1，Store 初始引用）。
/// - `retain()` 增加引用计数（snapshot/动画帧持有额外引用），更新 epoch 为当前值。
/// - `release()` 减少引用计数，带 epoch 校验。refcount 归零时 delete layout 并回收槽。
/// - `is_valid()` 通过 `get_paragraph_layout_checked` 校验 epoch，防止旧帧误取同槽新对象。
///
/// # 线程安全
///
/// 仅在 GUI 线程中使用，不跨线程共享。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QtLayoutHandle {
    /// QTextLayout 所在的 generation cache 代。
    pub generation: u64,
    /// QTextLayout 在 `g_layout_generations[generation]` 中的物理数组下标。
    pub physical_slot: i32,
    /// 该槽的 epoch — 同一物理槽被复用时递增，旧帧持有的旧 epoch 不匹配。
    pub epoch: u64,
}

impl QtLayoutHandle {
    /// 创建一个新的 `QtLayoutHandle`。
    pub fn new(generation: u64, physical_slot: i32, epoch: u64) -> Self {
        Self {
            generation,
            physical_slot,
            epoch,
        }
    }

    /// 是否表示无效/未分配的句柄。
    pub fn is_invalid(&self) -> bool {
        self.generation == 0 || self.physical_slot < 0
    }

    /// Issue #901 第二十九轮复核阻塞3: 增加布局引用计数，更新 epoch 为当前值。
    ///
    /// 用于 snapshot/动画帧持有额外引用。调用后 `self.epoch` 更新为 C++ 端当前 epoch，
    /// 供后续 [`release`] 校验。
    ///
    /// SAFETY: GUI thread only; generation/physical_slot 应是之前
    /// acquire_physical_slot_for_generation 分配的。
    pub fn retain(&mut self) {
        if self.is_invalid() {
            return;
        }
        self.epoch = super::qt_cache::add_layout_ref(self.generation, self.physical_slot);
    }

    /// Issue #901 第二十九轮复核阻塞3: 减少布局引用计数，带 epoch 校验。
    ///
    /// - epoch 不匹配：旧帧释放旧 handle（slot 已被复用，epoch 递增），直接忽略。
    /// - epoch 匹配：减 refcount。refcount 归零时 delete layout 并回收槽。
    ///
    /// SAFETY: GUI thread only; generation/physical_slot/epoch 应是之前
    /// acquire + install 或 retain 返回的。
    pub fn release(&self) {
        if self.is_invalid() {
            return;
        }
        super::qt_cache::release_layout_ref(self.generation, self.physical_slot, self.epoch);
    }

    /// Issue #901 第二十九轮复核阻塞3: 校验 epoch 并查询 layout 是否存在。
    ///
    /// 通过 C++ `get_paragraph_layout_checked(gen, slot, epoch)` 校验 epoch，
    /// 防止旧帧误取同槽新对象。返回 `true` 表示 layout 存在且 epoch 匹配。
    ///
    /// SAFETY: GUI thread only.
    pub fn is_valid(&self) -> bool {
        if self.is_invalid() {
            return false;
        }
        super::qt_cache::is_layout_valid(self.generation, self.physical_slot, self.epoch)
    }
}

/// Issue #658 评论 5624570557 问题 1: 已准备布局的只读句柄。
///
/// 暴露当前有效 `layout_generation + LayoutSnapshot` 的只读句柄，
/// 用于动画 old 帧从已有 QTextLine 提取视觉资源，不再重新排版。
/// 句柄持有者必须保证 generation 在句柄使用期间有效（不被 clear_layout_generation 释放）。
pub struct PreparedLayoutHandle<'a> {
    pub generation: u64,
    pub lines: &'a [VisualLine],
}
