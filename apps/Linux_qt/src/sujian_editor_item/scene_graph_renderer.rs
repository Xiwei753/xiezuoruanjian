use super::qt_text_node;
use super::render_plan::RenderPlan;
use super::texture_cache::TextureCache;
use crate::editor::layout::{LayoutSnapshot, ParagraphHeightIndex};
use crate::editor::scene_graph;

/// Issue #904：`render_frame` 的渲染结果。
///
/// 静态 QSGTextNode 负责正文的常驻绘制，本地 `static_holes`（动画覆盖层的孔洞）
/// 决定挖空哪些局部区域；动画覆盖层与光标/选区在本帧原子叠加。
/// 不记录整屏像素贡献——没有 PresentedScene 式第二套状态。
#[derive(Clone, Debug)]
pub(crate) struct FrameRenderOutcome {
    /// 静态层 rebuild 是否成功。
    pub static_rebuild_ok: bool,
    /// Issue #901 第九轮阻塞4: 本帧 rebuild 成功后实际加载的段落索引范围。
    /// 调用方将其保存为 per-item state，下一帧用此范围判断视口是否跨出
    /// 已加载集合。`None` 表示本帧未 rebuild 或 rebuild 失败，保留旧范围。
    pub new_loaded_para_range: Option<(usize, usize)>,
    /// Issue #904 评论 4：本帧**真正提交到 QSG** 的局部动画片段。
    ///
    /// 只有静态层提交成功、纹理存在且 C++ 侧真的建立并绑定了有效 `QSGTexture`
    /// 的片段才在这里；调用方据此记录“上一次真正画出的局部图片”，供快速连续
    /// 输入从当前画面重定向。计划侧（`TextMotionFrame`）不算数——它不代表像素
    /// 真的画出来了。
    pub committed_local: Vec<super::text_motion::VisibleFragment>,
    /// Issue #904 评论 4：动画层本帧是否与静态层一致成立。
    ///
    /// `true` 表示本帧静态层与动画层来自同一份事实：要么静态层按被接受的
    /// 孔洞挖空且动画纹理真的绑定成功，要么静态层画完整 canonical 且旧动画层
    /// 已清掉。`false` 表示两层都没能在本帧更新（保留上一帧的组合）。
    pub animation_layer_ok: bool,
    /// Issue #904 复核 5：本帧静态层**实际挖出**的孔洞（成功提交的静态 QSG
    /// 事实），不是计划侧的 `LocalTextPlan::static_holes`。
    ///
    /// 调用方据此记录“上一帧真的挖了孔”，用于下一帧判断是否需要强制刷新静态
    /// 裁剪：Qt 首帧拒绝纹理时这里为空，下一帧纹理可用时必须因此补挖孔。
    pub committed_holes: Vec<qt_text_node::AnimationClipRect>,
}

impl FrameRenderOutcome {
    /// 失败时的默认结果：rebuild 失败，无有效裁剪，无绘制片段。
    fn failed() -> Self {
        Self {
            static_rebuild_ok: false,
            new_loaded_para_range: None,
            committed_local: Vec::new(),
            animation_layer_ok: false,
            committed_holes: Vec::new(),
        }
    }
}

/// Issue #904 复核 5：`prepare_text_animation_layer` 的结果（两阶段提交的第一阶段）。
///
/// - `prepared`：本帧动画层是否预备成功（纹理全部就绪，或明确为空层）。
///   只有它为真时调用方才挖孔；它在 commit 之前不影响场景图。
/// - `committed`：预备成功时将要提交的局部片段（唯一事实来源）。
/// - `holes`：预备成功时本帧静态层应挖掉的孔洞。宽高为 0 的吐字首帧
///   `committed` 为空但 `holes` 仍然成立。
#[derive(Default)]
struct AnimationLayerPrep {
    prepared: bool,
    committed: Vec<super::text_motion::VisibleFragment>,
    holes: Vec<qt_text_node::AnimationClipRect>,
}

/// Issue #901 第九轮阻塞4: per-item 已加载段落索引区间。
///
/// 替代原全局 `static LOADED_PARA_RANGE`。`render_frame` 每次 rebuild 成功后
/// 把本帧视口（±一屏边距）对应的段落索引范围 `[start, end)` 通过
/// `FrameRenderOutcome::new_loaded_para_range` 返回给调用方。调用方将其保存为
/// per-item state，下一帧传入 `StaticTextParams::loaded_para_range`。
///
/// 纯滚动帧用此范围判断视口是否跨出已加载集合
/// （`new_start < loaded_start || new_end > loaded_end`），跨出则触发 rebuild，
/// 否则只更新 `QSGTransformNode` 位移矩阵。

/// Issue #904：动画覆盖层不再从整章 `CanvasDrawPlan` 推导，而是由
/// `text_motion::TextMotion::sample()` 直接产出局部 `TextAnimationGlyphInfo`。
/// 静态排除孔洞同样由 `TextMotionFrame.static_holes` 提供，不再有整屏
/// pixel-contribution 归属表。

/// 静态正文层的渲染参数 — 交给 QSGTextNode（Qt 6.7+ 公开 API）。
///
/// `layout_snapshot` 提供已排好的 VisualLine 数据（唯一 canonical 排版源）；
/// `needs_relayout` 控制是否重新排版（正文/字体/宽度变更时为 true，滚动时为 false）。
///
/// Issue #901 第六轮阻塞5: `height_index` 提供段落高度索引，用于 O(log n) 定位
/// 视口内首末可见段落，避免遍历全章 lines。`None` 表示无可用索引（fallback 到
/// lines 的 y 二分查找，仍为 O(log n)）。
pub(crate) struct StaticTextParams<'a> {
    pub layout_snapshot: Option<&'a LayoutSnapshot>,
    pub height_index: Option<&'a ParagraphHeightIndex>,
    pub scroll_y: f64,
    pub viewport_height: f64,
    pub color: &'a str,
    pub needs_relayout: bool,
    /// Issue #901 第十四轮问题2: 真正全局变化（字体/宽度/字号变化）时为 true。
    ///
    /// 与 `needs_relayout` 区分：`needs_relayout` 在普通正文输入时也为 true，
    /// 但普通正文输入只影响个别段落，不应 `force_full_update=true` 无差别重建所有段落。
    /// `force_full_relayout=true` 时 C++ 侧无条件重建所有已有段落子节点。
    pub force_full_relayout: bool,
    /// Issue #904 评论 1: 局部动画的静态挖孔状态本帧发生变化（接管开始/结束/目标变化）。
    ///
    /// 即使 `needs_relayout=false`（正文未变、布局未变），也必须重建静态节点的裁剪，
    /// 让 canonical 在动画结束的同一帧恢复显示、在动画开始的同一帧被挖空。
    /// 静态裁剪只在动画 begin/target-change/end 变化，不随 progress 每帧刷新。
    pub force_static_clip_refresh: bool,
    /// Issue #904 复核 1: 本帧整体段落 y 偏移 `(paragraph_id, 额外偏移)`。
    ///
    /// 换行/回流使下游段落下移时，复用已有 `QSGTransformNode` 从旧 y 插值到新 y；
    /// progress 走完时为空，段落直接留在目标矩阵。
    pub paragraph_y_offsets: &'a [(u64, f64)],
    pub session_id: u64,
    /// Issue #901 第九轮阻塞4: 上次 rebuild 成功后加载的段落索引范围。
    /// `None` 表示尚未加载任何段落（首次渲染或上次 rebuild 失败）。
    pub loaded_para_range: Option<(usize, usize)>,
}

/// Issue #901 第七轮复核问题2: 计算段落真实第一行的文档 y 坐标。
///
/// `QSGTextNode::addTextLayout(QPointF(origin_x, y), cachedLayout)` 消费完整段落的
/// QTextLayout，布局内每条 QTextLine 已相对段落第一行定位。视口裁剪只决定哪些
/// QTextLine 参与绘制，绝不能用"第一条可见行"的 y 冒充整个段落的起点。
///
/// # 实现策略
///
/// - 有 `ParagraphHeightIndex` 且 `logical_slot >= 0`：用 `paragraph_top_y(logical_slot)`
///   （O(log n) 前缀和）。`logical_slot` 与 heights 数组索引一致：
///   `extract_paragraph_heights` 按 `para_start` 首次出现顺序（= 文档段落顺序）构建，
///   `full.rs` 按 `paragraph_idx` 递增分配 `logical_slot`（含空段落），两者一致。
///   `paragraph_top_y` 不含 padding，需补 `snapshot.padding`（`VisualLine.y` 从
///   padding 起累加，见 `full.rs`）。
/// - 无 height_index 或 `logical_slot < 0`：fallback 到 lines 二分查找段落第一行。
///   lines 按 y 升序且同段落行连续（文档顺序），`partition_point(|l| l.para_start < ps)`
///   返回首个 `para_start >= ps` 的行 = 该段落第一行，其 y 已含 padding。
///
/// 不用 `unwrap`/`expect`：所有越界路径回退到 `line.y`，保证不 panic。
fn paragraph_top_document_y(
    line: &crate::editor::layout::VisualLine,
    snapshot: &LayoutSnapshot,
    height_index: Option<&ParagraphHeightIndex>,
) -> f64 {
    if let Some(hi) = height_index {
        if line.logical_slot >= 0 {
            // paragraph_top_y 越界返回总高度（不 panic），加 padding 仍是合法文档 y。
            return hi.paragraph_top_y(line.logical_slot as usize) + f64::from(snapshot.padding);
        }
    }
    // Fallback: lines 二分查找段落第一行。谓词 `para_start < ps` 为 true 前缀
    // （更早段落）、false 后缀（本段及之后），返回首个 false = 本段第一行索引。
    // Issue #901 第十轮问题5: 用 resolved_lines 按需物化后缀段绝对坐标。
    // TODO(issue-901): 后续优化为按需查询 — 此处是 fallback 路径（无 height_index），
    // 需在物化行上做 partition_point，难以简单改为按需物化。正常路径有 height_index 不会走到这里。
    let resolved_lines = snapshot.resolved_lines();
    let lines_slice = resolved_lines.as_ref();
    let first_idx = lines_slice.partition_point(|l| l.para_start < line.para_start);
    if first_idx < lines_slice.len() {
        lines_slice[first_idx].y
    } else {
        // 防御性回退：line 自身应在 lines 中，first_idx 应 < len；
        // 仅在 lines 为空或异常时回退到 line.y，避免越界。
        line.y
    }
}

/// Issue #901 第八轮问题3：计算当前视口（±一屏边距）对应的稳定段落索引范围。
///
/// 返回 `Some((start, end))` 表示段落 `[start, end)` 应被加载到 QSG 静态层；
/// 返回 `None` 表示无可用 `height_index` 或 `layout_snapshot`，调用方应跳过
/// 跨出检测（退化为旧行为）。
///
/// 视口边距与 `render_frame` rebuild 分支保持一致：`margin = viewport_height`，
/// `visible_top = scroll_y - margin`，`visible_bottom = scroll_y + viewport_height + margin`。
/// 这样"已加载范围"与 rebuild 实际建立的段落集合一致，跨出判断才准确。
///
/// 用 `ParagraphHeightIndex::paragraphs_in_y_range` 做 O(log n) 段落枚举，
/// 不遍历全章 lines。
fn compute_viewport_para_range(static_text: &StaticTextParams<'_>) -> Option<(usize, usize)> {
    let hi = static_text.height_index?;
    // layout_snapshot 不参与段落枚举（paragraphs_in_y_range 只用 height_index），
    // 但调用方通常同时提供两者。此处不强制要求 layout_snapshot。
    let viewport_top = static_text.scroll_y;
    let viewport_bottom = static_text.scroll_y + static_text.viewport_height;
    let margin = static_text.viewport_height.max(100.0);
    let visible_top = viewport_top - margin;
    let visible_bottom = viewport_bottom + margin;
    let (first_para, last_para_excl) = hi.paragraphs_in_y_range(visible_top, visible_bottom);
    Some((first_para, last_para_excl))
}

pub(crate) fn render_frame(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    static_text: &StaticTextParams<'_>,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
) -> FrameRenderOutcome {
    if root_raw.is_null() || item_ptr.is_null() {
        return FrameRenderOutcome::failed();
    }

    // Issue #668 评论 5646458592 问题 1: 静态正文 rebuild 的成功/失败结果。
    // 默认 true 表示无需 rebuild（如滚动帧）或 rebuild 成功；
    // false 表示 rebuild 因 layout 缺失而放弃，调用方应保留 dirty 标志。
    let mut static_rebuild_ok = true;
    // Issue #901 第九轮阻塞4: 本帧 rebuild 成功后的新加载段落范围，
    // 通过 FrameRenderOutcome 返回给调用方。
    let mut rebuild_new_range: Option<(usize, usize)> = None;

    // Layer 0: 静态正文 — QSGTextNode (Qt 6.7+ public API)
    // 消费 EditorLayout 唯一 canonical 排版结果，不再自行创建第二套 QTextLayout。
    //
    // Issue #714 评论 5740007764: 静态层在 needs_relayout=true 时重建。
    // needs_relayout 由 layout_dirty || scene_dirty 驱动，覆盖：
    //   - 正文/layout/颜色变化（layout_dirty=true，GUI 线程 request_static_repaint）
    //   - 正文动画开始/结束（scene_dirty=true）
    //
    // Issue #853：`clip_rects` 是动画层接管的完整 target cluster exclusion，
    // 在过渡期间不随 progress 改变。静态层只在所有权集合改变时重建；
    // glyph 切片由动画层逐帧更新。
    // ownership 不变的帧（纯滚动、纯光标动画、正文 progress）只更新轻量节点。
    //
    // `clip_rects` 表达静态层让给动画 owner 的目标 cluster。静态正文仍来自最新
    // canonical layout；动画层在这些位置绘制唯一的当前视觉。
    //
    // 先检查整份 ownership plan 的动画资源。缺一张就按 canonical 静态正文提交，
    // 不能只过滤静态 clip、再让动画层单独跳过缺纹理的 glyph。
    let animation_resources_ready = plan.local_text.has_animation_resources(texture_cache);

    // Issue #901 第八轮问题3：计算新视口对应的段落范围，与已加载范围比较。
    // 跨出已加载集合时触发 rebuild（insert 新段落节点 + remove 离开段落节点），
    // 不只平移旧一屏的 transform。这修复长距离滚动后新区域空白的问题。
    let new_para_range = compute_viewport_para_range(static_text);
    // Issue #901 第九轮阻塞4: 使用 per-item state 而非全局 static。
    let loaded_para_range = static_text.loaded_para_range.unwrap_or((0, 0));
    // 视口跨出已加载段落集合：新范围的下界 < 已加载下界，或新范围上界 > 已加载上界。
    // 无 new_para_range（无 height_index）时不触发跨出检测，退化为旧行为。
    let viewport_exceeds_loaded = match new_para_range {
        Some((new_start, new_end)) => {
            new_start < loaded_para_range.0 || new_end > loaded_para_range.1
        }
        None => false,
    };

    let should_rebuild_static = static_text.needs_relayout
        || static_text.force_static_clip_refresh
        || !animation_resources_ready
        || viewport_exceeds_loaded;

    // Issue #904 评论 4：静态层本轮必然失败（缺少与本帧一致的 prepared layout）时
    // 不触碰任何一层，保留上一帧的 static + animation 组合（与旧行为一致）。
    let static_snapshot_missing = should_rebuild_static && static_text.layout_snapshot.is_none();

    // Issue #904 复核 5：**先预备动画层**（只建/检查纹理，不改动动画节点），
    // 拿到 C++ 侧真正可用的纹理结果；随后用它给出的孔洞更新静态层；只有静态层
    // 也提交成功时才真正 commit 动画层。两层来自同一份事实（C++ receipt），
    // 原子交接：
    //   - 预备成功 + 静态成功 → 挖孔 + 动画同帧成立；
    //   - 预备失败           → 静态层画完整 canonical（不挖孔），再清掉旧动画层；
    //   - 静态提交失败       → 丢弃预备结果，保留上一帧 static + animation 组合。
    // 这是 render_frame 内部的顺序调整，不是第二套状态机。
    let mut prep = AnimationLayerPrep::default();
    if animation_resources_ready && !static_snapshot_missing {
        prep = prepare_text_animation_layer(
            root_raw,
            item_ptr,
            plan.local_text.overlays.as_slice(),
            plan.local_text.static_holes.as_slice(),
            texture_cache,
        );
    }

    if should_rebuild_static {
        // 正文/layout/颜色变化 或 活动事务集合变化：重建静态节点（含裁剪）
        if let Some(snapshot) = static_text.layout_snapshot {
            // Issue #901 第十五轮问题5b: 用 resolve_viewport_lines 只物化视口附近行，
            // 不物化整章。原实现用 resolved_lines() 物化整章再 partition_point 定位
            // 视口行范围，每帧 O(整章) 物化。现改为用 height_index 段落级视口枚举 +
            // cache_slot 二分定位行范围，只物化可见段内行，O(视口内行数) 物化。
            //
            // Issue #901 第五轮阻塞5：只遍历视口内的行，不全文重建。
            // 长章中只有视口附近的行需要处理，成本与视口大小相关，
            // 而不是和全章长度相关。扩展一个视口高度的 margin 避免滚动时
            // 频繁重建边界段落。
            let viewport_top = static_text.scroll_y;
            let viewport_bottom = static_text.scroll_y + static_text.viewport_height;
            let margin = static_text.viewport_height.max(100.0);
            let visible_top = viewport_top - margin;
            let visible_bottom = viewport_bottom + margin;

            // Issue #901 第十五轮问题5b: resolve_viewport_lines 内部用
            // height_index.paragraphs_in_y_range 做 O(log n) 段落枚举，
            // 再用 cache_slot 二分定位行范围，只物化可见段内行。
            // 无 height_index 时退化为物化所有行（与原 resolved_lines 一致）。
            let resolved_lines: Vec<crate::editor::layout::VisualLine> =
                snapshot.resolve_viewport_lines(static_text.scroll_y, static_text.viewport_height);
            let lines_slice = &resolved_lines[..];

            // Issue #901 第六轮阻塞5: 合并原三遍 for 循环为一遍。
            // 原实现分别遍历 lines 构建 paragraphs、para_y_map、visual_line_clips，
            // 每遍都是 O(全章行数)。现在只遍历视口附近行一次，
            // 同时构建三个集合，成本 O(视口内行数)。
            //
            // Issue #658: 按段落分组 VisualLine，每个段落对应一个 cache_idx。
            // cache_idx 直接从 VisualLine.cache_slot 读取（由 layout 阶段按段落出现顺序
            // 分配的稳定 slot），不再自行计数。
            let mut paragraphs: Vec<qt_text_node::ParagraphLineInfo> = Vec::new();
            let mut visual_line_clips: Vec<qt_text_node::VisualLineClipInfo> = Vec::new();
            // 段落第一行 y 的映射：para_start -> para_y
            let mut para_y_map: std::collections::HashMap<usize, f64> =
                std::collections::HashMap::new();
            let mut last_para_start: Option<usize> = None;
            // Issue #901 第十轮问题4: 维护当前段落的 ParagraphId，
            // 供 visual_line_clips 填入 paragraph_id 字段。
            let mut current_paragraph_id: u64 = 0;

            for line in lines_slice {
                // 跳过完全在扩展视口外的行（首尾段落可能含视口外行）。
                if line.y + line.height < visible_top || line.y > visible_bottom {
                    continue;
                }

                // Issue #901 第七轮复核问题2: para_y 必须是段落真实第一行文档 y，
                // 不能用"第一条可见行"的 y 冒充。QSGTextNode::addTextLayout 消费
                // 完整段落的 QTextLayout，布局内每条 QTextLine 已相对段落第一行定位；
                // 若视口从段落中间开始，用可见首行 y 会使前面被裁掉行的本地偏移
                // 重复累加，导致文字与命中/光标错位。同段落只计算一次并缓存。
                let para_y = *para_y_map.entry(line.para_start).or_insert_with(|| {
                    paragraph_top_document_y(line, snapshot, static_text.height_index)
                });

                if last_para_start != Some(line.para_start) {
                    // 新段落开始
                    last_para_start = Some(line.para_start);
                    // Issue #901 第二十轮阻塞3: 用 line.paragraph_id 直接获取稳定身份，
                    // 不再通过 cache_slot 查 paragraph_ids 表。
                    // 无 paragraph_id（INVALID）时用 0，不影响渲染正确性。
                    let paragraph_id =
                        if line.paragraph_id != crate::editor::layout::ParagraphId::INVALID {
                            line.paragraph_id.as_u64()
                        } else {
                            0
                        };
                    current_paragraph_id = paragraph_id;
                    // Issue #901 第十四轮问题2: 计算 per-paragraph content_revision。
                    // 用 paragraph_text 的哈希值作为内容版本签名，
                    // 与 ParagraphLayoutCacheKey::text_hash 一致。
                    let content_revision = {
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        line.para_text.hash(&mut hasher);
                        hasher.finish()
                    };
                    // Issue #901 第十四轮问题2: 计算段落总高度，供 C++ 侧过滤 clip。
                    // 有 height_index 时用 O(1) 前缀和差，否则用单行高度作 fallback。
                    // Issue #901 第二十轮阻塞3: 用 logical_slot 查 height_index，
                    // 不用 cache_slot（Qt 物理槽，与 height_index 索引不对应）。
                    let paragraph_height = if let Some(hi) = static_text.height_index {
                        if line.logical_slot >= 0 {
                            let slot = line.logical_slot as usize;
                            hi.paragraph_bottom_y(slot) - hi.paragraph_top_y(slot)
                        } else {
                            line.height
                        }
                    } else {
                        line.height
                    };
                    paragraphs.push(qt_text_node::ParagraphLineInfo {
                        paragraph_text: line.para_text.clone(),
                        para_start: line.para_start,
                        paragraph_id,
                        cache_idx: line.cache_slot,
                        // Issue #901 第三十轮问题1: 传入 layout_epoch，
                        // 供 C++ 侧用 get_paragraph_layout_checked 校验 epoch。
                        layout_epoch: line.layout_epoch,
                        y: para_y,
                        indent_w: line.para_indent,
                        font_size: snapshot.font_size,
                        font_family: snapshot.font_family.clone(),
                        doc_width: snapshot.width,
                        content_revision,
                        paragraph_height,
                    });
                }

                // Issue #658: 构建 per-visual-line 裁剪数据，用于按视觉行裁剪动画接管区域。
                // 每个视觉行携带自己的 cache_slot、qtextline_idx、y、height、x、width，
                // 以及所属段落的 para_y（addTextLayout 偏移）。
                // Issue #901 第十轮问题4: 填入 paragraph_id，供 C++ 侧按 ParagraphId
                // 分组 visual lines 到正确的段落容器节点。
                visual_line_clips.push(qt_text_node::VisualLineClipInfo {
                    cache_idx: line.cache_slot,
                    // Issue #901 第三十轮问题1: 传入 layout_epoch，
                    // 供 C++ 侧用 get_paragraph_layout_checked 校验 epoch。
                    layout_epoch: line.layout_epoch,
                    qtextline_idx: line.qtextline_idx,
                    y: line.y,
                    height: line.height,
                    x: line.x,
                    width: line.width,
                    para_y,
                    doc_width: snapshot.width,
                    paragraph_id: current_paragraph_id,
                    // Issue #901 第三十一轮问题1: 空段（cache_slot < 0）无 QTextLayout，
                    // has_qt_layout = false，C++ 渲染路径跳过 layout 查询。
                    // 非空段 has_qt_layout = true，走 get_paragraph_layout_checked。
                    has_qt_layout: line.cache_slot >= 0,
                });
            }

            // Issue #853: 静态 exclusion 直接来自统一 owner table。
            // 不再通过 StaticLinePatch 中间结构换算。
            // Issue #736 评论 5786231506: 静态层开始裁剪之前先检查本帧动画所需的
            // snapshot texture。缺纹理的 snapshot 对应 clip 不进入 static text clip，
            // canonical 正文直接显示。不能等静态层挖完以后到了动画层才 continue。
            // overlay 可画 -> static 被接管；overlay 不可画 -> static 同帧
            // 恢复 canonical。两边是一条原子规则。
            //
            // Issue #904 复核 5: 孔洞取「动画层预备成功时才成立」的集合
            // （`prep.holes`）。预备失败时为空 —— 静态层画完整 canonical，
            // 随后清掉旧动画层，与动画层同一份事实原子交接。
            let available_clip_rects = qt_text_node::merge_static_clip_rects(prep.holes.clone());
            let clip_rects = &available_clip_rects;

            // Issue #658 评论 5620035970 问题 4: 正文从 padding 开始画，
            // origin_x = snapshot.padding，与 VisualLine.x = padding + x_off 一致。
            // Issue #658 评论 5620035970 问题 2: 传 snapshot.layout_generation，
            // 用 (generation, cache_slot) 查找 layout，与动画/IME 路径互不干扰。
            // Issue #668: 接住 rebuild 的成功/失败结果，透传给 qquickitem_impl。
            //
            // Issue #901 第十轮问题4: 改为调用 update_text_nodes_incremental
            // 做增量 insert/remove/update，而非全量 rebuild。
            //
            // Issue #901 第十四轮问题2: force_full_update 判断修正：
            // - force_full_relayout: 真正全局变化（字体/宽度/字号）→ 重建所有已有段落
            // - !animation_resources_ready: 资源缺失，从有 clip 到无 clip 或反之 → 重建所有
            // - needs_relayout（普通正文输入）: 不再 force_full_update=true，
            //   由 C++ 侧 per-paragraph content_revision 独立判断哪些段变化
            // - viewport_exceeds_loaded（纯滚动跨出）: 段落内容不变 → 只 insert/remove
            //
            // Issue #904：clip 集合只影响静态层的挖孔，不影响段内容/布局。
            // clip 子节点的更新由 update_text_nodes_incremental 内部根据
            // animation_clip_rects 参数独立完成，不依赖 force_full_update。
            //
            // Issue #901 第十三轮问题3+4：force_full_update=false 时不再无差别 continue，
            // 而是由 update_text_nodes_incremental 内部按 per-paragraph content_revision
            // 和 per-paragraph clip signature 比较驱动：
            // - para_y 变化 → 只更新 transform_node 矩阵（不重建子节点）
            // - content_revision 变化 → 重建子节点（内容变了）
            // - clip signature 变化 → 重建子节点（裁剪变了）
            // - 都不变 → 真正跳过
            //
            // Issue #904 评论 2: 全量重建只用于真正全局变化（字体/宽度/字号），
            // 那会生成整棵新树并原子替换。普通正文输入（needs_relayout）走
            // per-paragraph 增量：以 (content_revision, cache_idx, epoch) 为内容身份，
            // 在 detached 新节点上构建成功后才用 insertChildNodeBefore 原子替换。
            // 增量失败时保留旧树，绝不 clear 整棵树再全量重建（那正是正文消失的根因）。
            if static_text.force_full_relayout {
                static_rebuild_ok = qt_text_node::rebuild_text_node_from_paragraphs(
                    root_raw,
                    item_ptr,
                    &paragraphs,
                    &visual_line_clips,
                    static_text.scroll_y,
                    static_text.color,
                    clip_rects,
                    f64::from(snapshot.padding),
                    snapshot.layout_generation,
                );
            } else {
                static_rebuild_ok = qt_text_node::update_text_nodes_incremental(
                    root_raw,
                    item_ptr,
                    &paragraphs,
                    &visual_line_clips,
                    static_text.scroll_y,
                    static_text.color,
                    clip_rects,
                    static_text.paragraph_y_offsets,
                    f64::from(snapshot.padding),
                    snapshot.layout_generation,
                    false,
                    static_text.session_id,
                );
            }

            // Issue #901 第九轮阻塞4: rebuild 成功后记录本帧实际加载的段落范围，
            // 通过 FrameRenderOutcome 返回给调用方保存为 per-item state。
            // rebuild 失败时不更新（返回 None，调用方保留旧范围，下一帧重试）。
            rebuild_new_range = if static_rebuild_ok {
                new_para_range
            } else {
                None
            };
        } else {
            // 没有与本帧一致的 prepared layout 时不能提交新的 owner 状态。
            static_rebuild_ok = false;
        }
    } else {
        // 纯滚动帧 或 动画 progress 变化帧：只更新 QSGTransformNode 位移矩阵，
        // 不重建静态节点。不出现 clear()、QTextLayout、createLine()、addTextLayout()。
        // Issue #714 评论 5740007764: 动画期间静态层保持上一帧（事务开始时重建的）
        // 裁剪后节点不动，避免每帧销毁/重建静态 QSGTextNode 产生闪烁。
        // Issue #901 问题5缺陷A: 此 else 分支是"本帧正文未变跳过重建"的优化点。
        // should_rebuild_static=false 意味着 needs_relayout=false 且动画资源就绪
        // 且视口未跨出已加载段落集合，即本帧只有滚动位移或动画
        // progress 变化，正文内容未变且新视口仍在上次 rebuild 加载的段落范围内。
        // 此时跳过 rebuild_text_node_from_paragraphs 全量重建，只平移 transform，
        // 成本 O(1) 而非 O(视口内段落数)。
        // Issue #901 第八轮问题3: 视口跨出已加载集合的情况已在上方
        // should_rebuild_static 判断中被 viewport_exceeds_loaded 捕获，走 rebuild
        // 分支按需 insert/remove 段落节点。进入此 else 分支 guarantee 视口未跨出，
        // 平移旧节点集合是安全的（所有可见段落都已在 QSG 中有节点）。
        qt_text_node::update_scroll_transform(root_raw, static_text.scroll_y);
    }

    // Layer 1: 文字动画层（保留：吐字/吞字/重排动画的纹理切片）
    // Issue #709 评论 issue-body-709: 动画层单独画 InsertReveal / DeleteConceal / ReflowMove，
    // 不把静态正文重新补回去。静态层在 clip_count > 0 时已通过 complement 区间
    // 把被动画接管的区域从静态正文里挖掉，动画层只负责画那些被接管的区域。
    // 两层在文档区域上互斥，避免静态正文盖住吐字/吞字动画。
    // Issue #715: 传 scroll_y 给动画层，让 AnimationLayerNode 设置和静态层一样的
    // translate(0, -scroll_y) 矩阵。动画 glyph 继续保留文档坐标，不在 Rust 侧逐个减 scroll_y。
    //
    // Issue #904 复核 5: 动画层与静态孔洞的真正交接点。
    // 只有静态层也提交成功、或本帧根本不需要重建静态层时，才真正替换动画节点；
    // 静态层失败时丢弃预备结果，保留上一帧的 static + animation 组合。
    let mut committed_local: Vec<super::text_motion::VisibleFragment> = Vec::new();
    let mut committed_holes: Vec<qt_text_node::AnimationClipRect> = Vec::new();
    let mut animation_layer_ok = false;
    if prep.prepared && static_rebuild_ok {
        if scene_graph::commit_animation_layer(root_raw, item_ptr, static_text.scroll_y) {
            animation_layer_ok = true;
            committed_holes = qt_text_node::merge_static_clip_rects(prep.holes);
            committed_local = prep.committed;
        }
    } else if prep.prepared {
        // 静态层提交失败：丢弃本帧预备的动画纹理/节点替换，旧动画节点保持原样，
        // 与旧静态正文仍属同一帧。绝不留下“新动画 + 旧正文”。
        scene_graph::discard_animation_layer(root_raw, item_ptr);
    } else if should_rebuild_static && static_rebuild_ok {
        // 动画层本帧不成立（资源缺失或纹理创建失败），静态层已按完整 canonical
        // 提交（无孔洞）。此刻才清掉旧动画层，两层同帧一致：没有孔洞、也没有
        // 残留在旧孔洞上方的动态图。
        scene_graph::clear_animation_layer(root_raw, item_ptr);
        animation_layer_ok = true;
    }
    // 其余情况（既不预备也没重建静态层）保留上一帧的 static + animation 组合。
    // Layer 2: 选区/预输入背景
    // Issue #677 评论 5654174714: scroll_y 作为每帧轻量状态传给 renderer，
    // selection/preedit 几何保持文档坐标，由 renderer 在绘制时做视口换算。
    // Issue #677 评论 5654686856: viewport_height 一并传入，renderer 做轻量视口裁剪。
    render_selection_preedit_layer(
        root_raw,
        item_ptr,
        plan,
        static_text.scroll_y,
        static_text.viewport_height,
    );
    // Layer 3: 光标
    // Issue #727 评论 5755858583 问题1: scroll_y 传给 render_cursor_layer，
    // cursor layer 的 QSGTransformNode 统一做 translate(0, -scroll_y)。
    render_cursor_layer(root_raw, item_ptr, plan, static_text.scroll_y);

    FrameRenderOutcome {
        static_rebuild_ok,
        new_loaded_para_range: rebuild_new_range,
        committed_local,
        committed_holes,
        animation_layer_ok,
    }
}

/// Issue #904 复核 5：**预备**动画层（两阶段提交的第一阶段，不改动场景图）。
///
/// 只把本帧需要的 `QSGTexture` 建好/取出并暂存，返回预备结果。真正的节点替换由
/// [`crate::editor::scene_graph::commit_animation_layer`] 在静态层也提交成功后执行；
/// 静态层失败时调用 `discard_animation_layer` 丢弃，旧动画节点不受影响。
///
/// 返回值同时给出本帧静态层应挖的 `holes`：预备成功时等于计划的
/// `static_holes`（含 progress=0 尚无可见像素、但已接管目标的吐字），
/// 预备失败时为空 —— 保证静态孔洞与动画纹理原子交接。
fn prepare_text_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    glyphs: &[super::render_plan::TextAnimationGlyphInfo],
    holes: &[qt_text_node::AnimationClipRect],
    texture_cache: &TextureCache,
) -> AnimationLayerPrep {
    // Issue #658 评论 5630650436: 空动画帧也要让 AnimationLayerNode 把旧 node 和
    // texture 一次清干净（走下面的 `glyph_count == 0` 分支 clear_animation_layer），
    // 否则 activeSet sweep 不运行，m_texture_cache 里的 QSGTexture 继续留着。
    let mut glyph_data: Vec<f64> = Vec::new();
    let mut glyph_images: Vec<qmetaobject::QImage> = Vec::new();
    let mut source_rects: Vec<f64> = Vec::new();
    let mut snapshot_ids: Vec<u64> = Vec::new();
    // Issue #904 评论 4：本帧候选提交出去的局部片段（与 C++ 接受的子集一一对应）。
    let mut committed_local: Vec<super::text_motion::VisibleFragment> = Vec::new();

    for glyph in glyphs.iter() {
        // Issue #736 评论 5778543593 修改1: 缺该 snapshot_id 的纹理时，这个
        // 文字动画 frame 不进入 glyph_data。缺纹理直接 continue 跳过整个
        // glyph，不再塞 1×1 空图把"DeleteConceal 已生成但纹理没拿到"伪装成
        // "动画层正常执行只是什么也看不到"。有纹理时才 push 四个 vec，这样
        // 四个 vec 长度始终一致，glyph_count == glyph_images.len() 自然通过。
        let texture = match texture_cache.get_line(&glyph.snapshot_id) {
            Some(texture) => texture,
            None => continue,
        };

        // Issue #901 问题4：用 clip_rect 对 target_rect 做预裁剪，并按比例
        // 调整 source_uv，使吞字/吐字的 reveal/conceal 裁剪真正作用到 Qt
        // 图形层。不需要改 C++ 签名 —— 裁剪后的 target_rect 和 source_uv
        // 直接传给 update_animation_layer。
        //
        // 矩形交集 + UV 映射：
        //   ix = max(tx, cx), iy = max(ty, cy)
        //   ix2 = min(tx+tw, cx+cw), iy2 = min(ty+th, cy+ch)
        //   new_target = (ix, iy, ix2-ix, iy2-iy)
        //   u_scale = sw/tw, v_scale = sh/th
        //   new_source = (sx + (ix-tx)*u_scale, sy + (iy-ty)*v_scale,
        //                 (ix2-ix)*u_scale, (iy2-iy)*v_scale)
        let tx = glyph.x;
        let ty = glyph.y;
        let tw = glyph.w;
        let th = glyph.h;
        let sx = glyph.source_rect.x;
        let sy = glyph.source_rect.y;
        let sw = glyph.source_rect.w;
        let sh = glyph.source_rect.h;
        let cx = glyph.clip_rect.x;
        let cy = glyph.clip_rect.y;
        let cw = glyph.clip_rect.w;
        let ch = glyph.clip_rect.h;

        let ix = tx.max(cx);
        let iy = ty.max(cy);
        let ix2 = (tx + tw).min(cx + cw);
        let iy2 = (ty + th).min(cy + ch);
        let iw = ix2 - ix;
        let ih = iy2 - iy;

        // 裁剪后无可见面积则跳过此 glyph。
        if iw <= 0.0 || ih <= 0.0 {
            continue;
        }

        // UV 按比例调整。tw/th 为 0 时不会到达这里（上面 iw/ih 判断已过滤），
        // 但防御性处理避免除零。
        let u_scale = if tw > 0.0 { sw / tw } else { 0.0 };
        let v_scale = if th > 0.0 { sh / th } else { 0.0 };
        let new_sx = sx + (ix - tx) * u_scale;
        let new_sy = sy + (iy - ty) * v_scale;
        let new_sw = iw * u_scale;
        let new_sh = ih * v_scale;

        glyph_data.extend_from_slice(&[ix, iy, iw, ih, glyph.opacity]);

        source_rects.extend_from_slice(&[new_sx, new_sy, new_sw, new_sh]);

        let cache_key = glyph.snapshot_id.to_cache_key();
        snapshot_ids.push(cache_key);
        glyph_images.push(texture.clone());

        // Issue #904 评论 4：这一片段确实以 (ix, iy, iw, ih) 提交出去了。
        // 当前位置取插值后的 target_rect（Move 会在途），当前可见宽度取裁剪后的 iw。
        committed_local.push(super::text_motion::VisibleFragment {
            snapshot: super::text_motion::SnapshotRef {
                snapshot_id: glyph.snapshot_id,
                source_uv: glyph.source_rect,
            },
            byte_range: glyph.logical_range,
            rect: super::layout_snapshot::SourceRect {
                x: glyph.x,
                y: glyph.y,
                w: glyph.w,
                h: glyph.h,
            },
            mask_width: iw,
            paragraph_id: glyph.paragraph_id,
            shaping: glyph.shaping.clone(),
            source_revision: glyph.source_revision,
            glyph_id: glyph.glyph_id,
        });
    }

    let glyph_count = glyph_data.len() / 5;
    if glyph_count == 0 {
        // Issue #904 复核 6：本帧没有可见 glyph（如吐字 progress=0 的 0 宽首帧、
        // 或动画结束帧 0 glyph + 0 hole）。这里**必须真的调用一次 prepare**：
        // C++ 只有被调用且 glyph_count<=0 时才置 `m_prepared_empty=true`；
        // 若在这里直接早退，随后的 commitPrepared 会因为既没有 `m_prepared`
        // 也没有 `m_prepared_empty` 而什么都不做，上一帧的旧动态图节点残留，
        // 静态正文恢复完整 canonical 后就叠字/重影。
        // 预备成功后孔洞照旧带回：吐字 0 宽首帧仍要把目标字形从静态层排除。
        let accepted = scene_graph::prepare_animation_layer(
            root_raw,
            item_ptr,
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        );
        return if accepted == 0 {
            AnimationLayerPrep {
                prepared: true,
                committed: Vec::new(),
                holes: holes.to_vec(),
            }
        } else {
            AnimationLayerPrep::default()
        };
    }

    // `glyph_count` 与 `glyph_images.len()` 在循环内同步 push，恒等；
    // 这里把 C++ 是否真正建立并绑定有效纹理作为唯一事实来源。
    let image_ptrs: Vec<*const qmetaobject::QImage> = glyph_images
        .iter()
        .map(|img| img as *const qmetaobject::QImage)
        .collect();

    let accepted = scene_graph::prepare_animation_layer(
        root_raw,
        item_ptr,
        glyph_count as i32,
        glyph_data.as_ptr(),
        image_ptrs.as_ptr(),
        source_rects.as_ptr(),
        snapshot_ids.as_ptr(),
    );

    if accepted == glyph_count as i32 {
        AnimationLayerPrep {
            prepared: true,
            committed: committed_local,
            holes: holes.to_vec(),
        }
    } else {
        // C++ 未能把本帧全部 glyph 建立并绑定有效纹理（root/动画层缺失或纹理
        // 创建失败）。丢弃预备结果，动画层本帧不成立：调用方不挖任何孔洞，
        // 并在静态层画完整 canonical 之后清掉旧动画层。
        scene_graph::discard_animation_layer(root_raw, item_ptr);
        AnimationLayerPrep::default()
    }
}

fn render_cursor_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    scroll_y: f64,
) {
    // Issue #679 评论 5657313927: 直接画 CursorRenderState，
    // 不再理解 Snap/Tween/driver/Timestamp。
    // Issue #727 评论 5755858583 问题1: cursor.y 是文档坐标，scroll_y 传给
    // update_cursor_node 由 QSGTransformNode 统一做视口变换。
    let cursor = &plan.cursor;
    let cursor_style = &plan.cursor_style;

    if !cursor.visible {
        scene_graph::update_cursor_node(
            root_raw,
            item_ptr,
            cursor.x,
            cursor.y,
            cursor_style.width,
            cursor.h,
            0.0,
            cursor_style.color.as_ptr(),
            cursor_style.color.len(),
            scroll_y,
        );
        return;
    }

    scene_graph::update_cursor_node(
        root_raw,
        item_ptr,
        cursor.x,
        cursor.y,
        cursor_style.width,
        cursor.h,
        cursor.opacity,
        cursor_style.color.as_ptr(),
        cursor_style.color.len(),
        scroll_y,
    );
}

fn render_selection_preedit_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    plan: &RenderPlan,
    scroll_y: f64,
    viewport_height: f64,
) {
    let sp = &plan.selection_preedit;
    let total_count = sp.selection_ranges.len() + sp.preedit_ranges.len();
    if total_count == 0 {
        scene_graph::update_selection_preedit_layer(root_raw, item_ptr, 0, std::ptr::null());
        return;
    }

    // Issue #677 评论 5654174714: 颜色是每帧轻量状态，从 RenderPlan.selection_preedit_style
    // 读取；带透明度的最终颜色在 renderer 内部计算（selection 0x33, preedit 0x1A），
    // 与原 build_selection_preedit_plan_from_snapshot 中的逻辑保持一致。
    let base_color = &plan.selection_preedit_style.selection_color;
    let (base_r, base_g, base_b) = if base_color.starts_with('#') && base_color.len() >= 7 {
        (
            f64::from(u8::from_str_radix(&base_color[1..3], 16).unwrap_or(0)) / 255.0,
            f64::from(u8::from_str_radix(&base_color[3..5], 16).unwrap_or(0)) / 255.0,
            f64::from(u8::from_str_radix(&base_color[5..7], 16).unwrap_or(0)) / 255.0,
        )
    } else {
        // 与原 fallback "#3381D1D1" / "#1A81D1D1" 的 RGB 部分一致。
        (
            0x81 as f64 / 255.0,
            0xD1 as f64 / 255.0,
            0xD1 as f64 / 255.0,
        )
    };
    let selection_alpha = 0x33 as f64 / 255.0;
    let preedit_alpha = 0x1A as f64 / 255.0;

    let mut rect_data: Vec<f64> = Vec::with_capacity(total_count * 10);

    // Issue #677 评论 5654686856: 每帧轻量视口裁剪，只跳过完全在屏幕外的矩形。
    // 滚动只改变轻量显示状态，不重新排版，也不会因为旧 frame 曾经裁掉某些行而丢选区
    // （因为 frame 现在保存完整文档坐标几何，见 build_selection_preedit_plan_from_snapshot）。
    for sel in &sp.selection_ranges {
        // Issue #677 评论 5654174714: sel.y 是文档坐标，绘制时减 scroll_y 得到视口坐标。
        let screen_y = sel.y - scroll_y;
        if screen_y + sel.h < 0.0 || screen_y > viewport_height {
            continue;
        }
        rect_data.extend_from_slice(&[
            sel.x,
            screen_y,
            sel.w,
            sel.h,
            base_r,
            base_g,
            base_b,
            selection_alpha,
            0.0,
            0.0,
        ]);
    }

    for pre in &sp.preedit_ranges {
        let screen_y = pre.y - scroll_y;
        if screen_y + pre.h < 0.0 || screen_y > viewport_height {
            continue;
        }
        let underline = if pre.underline { 1.0 } else { 0.0 };
        rect_data.extend_from_slice(&[
            pre.x,
            screen_y,
            pre.w,
            pre.h,
            base_r,
            base_g,
            base_b,
            preedit_alpha,
            underline,
            0.0,
        ]);
    }

    // Issue #677 评论 5654686856: 用 rect_data 实际长度计算 count，因为视口裁剪可能跳过部分矩形。
    let actual_count = rect_data.len() / 10;
    if actual_count == 0 {
        scene_graph::update_selection_preedit_layer(root_raw, item_ptr, 0, std::ptr::null());
    } else {
        scene_graph::update_selection_preedit_layer(
            root_raw,
            item_ptr,
            actual_count as i32,
            rect_data.as_ptr(),
        );
    }
}
