use super::qt_text_node;
use super::render_plan::{RenderPlan, TextAnimationGlyphInfo};
use super::texture_cache::TextureCache;
use super::visual_canvas::{CanvasDrawPlan, DrawCommandKind};
use crate::editor::layout::LayoutSnapshot;
use crate::editor::scene_graph;

/// Issue #901 阻塞 1：从 `CanvasDrawPlan` 的 `TextFragment` 命令转换为
/// `TextAnimationGlyphInfo` 列表，供动画层消费。
///
/// Issue #901 问题4：不再按 `opacity < 1.0` 过滤。存活字 FLIP 的 opacity=1
/// 也要被绘制。按 `DrawCommandKind::TextFragment` 和真正 owner/几何变化来绘制，
/// 不根据 alpha<1 决定"是否属于动画"。
///
/// Issue #901 问题2：`DrawCommand.target_rect` 和 `clip_rect` 始终使用文档坐标。
/// C++ 动画层（`AnimationLayerNode`）统一施加一次 `translate(0, -scroll_y)`
/// 变换（通过 `update_animation_layer` 的 `scroll_y` 参数）。
///
/// Issue #901 问题3：动画层的动态 clip 按 `progress` 更新（通过
/// `DrawCommand.target_rect` 和 `DrawCommand.clip_rect`），与静态层排除区
/// （`static_exclusion_rects`）是不同的两个概念。
pub(crate) fn canvas_draw_plan_to_glyph_infos(
    canvas_plan: &CanvasDrawPlan,
    _texture_cache: &TextureCache,
) -> Vec<TextAnimationGlyphInfo> {
    use super::layout_snapshot::{ShapingIdentity, SourceRect};
    use super::edit_motion::DeleteEdge;

    canvas_plan
        .commands
        .iter()
        .filter(|cmd| cmd.kind == DrawCommandKind::TextFragment && cmd.opacity > 0.0)
        .map(|cmd| {
            // Issue #901 问题2：直接使用 DrawCommand 携带的完整 LineSnapshotId，
            // 不再从 u64 hash 反解。动画层通过完整 LineSnapshotId 查询 texture_cache。
            let snapshot_id = cmd.snapshot_id;
            TextAnimationGlyphInfo {
                x: cmd.target_rect.x,
                y: cmd.target_rect.y,
                w: cmd.target_rect.w,
                h: cmd.target_rect.h,
                opacity: cmd.opacity,
                snapshot_id,
                source_rect: SourceRect {
                    x: cmd.source_uv.x,
                    y: cmd.source_uv.y,
                    w: cmd.source_uv.w,
                    h: cmd.source_uv.h,
                },
                // Issue #901 问题4：把 DrawCommand.clip_rect 传给动画 glyph，
                // 让 render_text_animation_layer 对 target_rect 和 source_uv
                // 做预裁剪，使吞字/吐字的 reveal/conceal 裁剪真正作用到 Qt 图形层。
                clip_rect: SourceRect {
                    x: cmd.clip_rect.x,
                    y: cmd.clip_rect.y,
                    w: cmd.clip_rect.w,
                    h: cmd.clip_rect.h,
                },
                logical_range: (0, 0),
                canonical_range: None,
                delete_edge: None,
                shaping_identity: ShapingIdentity {
                    text_content_hash: 0,
                    raw_font_fingerprint: String::new(),
                    glyph_indexes_hash: 0,
                    cluster_glyph_count: 0,
                    direction_rtl: false,
                    format_fingerprint: 0,
                },
            }
        })
        .collect()
}

/// Issue #901 问题3：从 `CanvasDrawPlan` 的 `static_exclusion_rects` 推导
/// 静态层裁剪矩形。
///
/// 静态层的 **target 字形排除区**覆盖本次由动画画布接管的**完整目标范围**，
/// 整个过渡期间与 `progress` 无关。只在接管集合改变时重建静态 QSGTextNode，
/// 不在每帧改变挖空矩形。
///
/// 动画层的动态 clip/UV 才按 `progress` 更新（在 `canvas_draw_plan_to_glyph_infos`
/// 中用 `DrawCommand.target_rect` 和 `DrawCommand.clip_rect`）。
///
/// Issue #901 问题2：`static_exclusion_rects` 使用文档坐标，与 `AnimationClipRect`
/// 的文档坐标契约一致。C++ 静态层（`QSGTransformNode`）统一施加一次
/// `translate(0, -scroll_y)` 变换。
pub(crate) fn canvas_draw_plan_to_clip_rects(
    canvas_plan: &CanvasDrawPlan,
) -> Vec<qt_text_node::AnimationClipRect> {
    canvas_plan
        .static_exclusion_rects
        .iter()
        .map(|excl| qt_text_node::AnimationClipRect {
            x: excl.rect.x,
            y: excl.rect.y,
            w: excl.rect.w,
            h: excl.rect.h,
            // Issue #901 问题2：直接使用完整 LineSnapshotId。
            snapshot_id: excl.snapshot_id,
        })
        .collect()
}

/// 静态正文层的渲染参数 — 交给 QSGTextNode（Qt 6.7+ 公开 API）。
///
/// `layout_snapshot` 提供已排好的 VisualLine 数据（唯一 canonical 排版源）；
/// `needs_relayout` 控制是否重新排版（正文/字体/宽度变更时为 true，滚动时为 false）。
pub(crate) struct StaticTextParams<'a> {
    pub layout_snapshot: Option<&'a LayoutSnapshot>,
    pub scroll_y: f64,
    pub viewport_height: f64,
    pub color: &'a str,
    pub needs_relayout: bool,
}

pub(crate) fn render_frame(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    static_text: &StaticTextParams<'_>,
    plan: &RenderPlan,
    texture_cache: &TextureCache,
) -> bool {
    if root_raw.is_null() || item_ptr.is_null() {
        return false;
    }

    // Issue #668 评论 5646458592 问题 1: 静态正文 rebuild 的成功/失败结果。
    // 默认 true 表示无需 rebuild（如滚动帧）或 rebuild 成功；
    // false 表示 rebuild 因 layout 缺失而放弃，调用方应保留 dirty 标志。
    let mut static_rebuild_ok = true;

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
    let animation_resources_ready = plan.ownership.has_animation_resources(texture_cache);
    let should_rebuild_static =
        static_text.needs_relayout || plan.ownership.handoff_pending || !animation_resources_ready;

    if should_rebuild_static {
        // 正文/layout/颜色变化 或 活动事务集合变化：重建静态节点（含裁剪）
        if let Some(snapshot) = static_text.layout_snapshot {
            // Issue #901 第五轮阻塞5：只遍历视口内的行，不全文重建。
            // 长章中只有视口附近的行需要处理，成本与视口大小相关，
            // 而不是和全章长度相关。扩展一个视口高度的 margin 避免滚动时
            // 频繁重建边界段落。
            let viewport_top = static_text.scroll_y;
            let viewport_bottom = static_text.scroll_y + static_text.viewport_height;
            let margin = static_text.viewport_height.max(100.0);
            let visible_top = viewport_top - margin;
            let visible_bottom = viewport_bottom + margin;

            // Issue #658: 按段落分组 VisualLine，每个段落对应一个 cache_idx。
            // cache_idx 直接从 VisualLine.cache_slot 读取（由 layout 阶段按段落出现顺序
            // 分配的稳定 slot），不再自行计数。
            let mut paragraphs: Vec<qt_text_node::ParagraphLineInfo> = Vec::new();
            let mut last_para_start: Option<usize> = None;

            for line in &snapshot.lines {
                // Issue #901 第五轮阻塞5：跳过完全在扩展视口外的行。
                if line.y + line.height < visible_top || line.y > visible_bottom {
                    continue;
                }
                if last_para_start != Some(line.para_start) {
                    // 新段落开始
                    last_para_start = Some(line.para_start);
                    paragraphs.push(qt_text_node::ParagraphLineInfo {
                        paragraph_text: line.para_text.clone(),
                        para_start: line.para_start,
                        cache_idx: line.cache_slot,
                        y: line.y,
                        indent_w: line.para_indent,
                        font_size: snapshot.font_size,
                        font_family: snapshot.font_family.clone(),
                        doc_width: snapshot.width,
                    });
                }
            }

            // Issue #658: 构建 per-visual-line 裁剪数据，用于按视觉行裁剪动画接管区域。
            // 每个视觉行携带自己的 cache_slot、qtextline_idx、y、height、x、width，
            // 以及所属段落的 para_y（addTextLayout 偏移）。
            let mut visual_line_clips: Vec<qt_text_node::VisualLineClipInfo> = Vec::new();
            // 段落第一行 y 的映射：para_start -> para_y
            let mut para_y_map: std::collections::HashMap<usize, f64> =
                std::collections::HashMap::new();
            for line in &snapshot.lines {
                // Issue #901 第五轮阻塞5：跳过完全在扩展视口外的行。
                if line.y + line.height < visible_top || line.y > visible_bottom {
                    continue;
                }
                para_y_map.entry(line.para_start).or_insert(line.y);
            }
            for line in &snapshot.lines {
                // Issue #901 第五轮阻塞5：跳过完全在扩展视口外的行。
                if line.y + line.height < visible_top || line.y > visible_bottom {
                    continue;
                }
                let para_y = *para_y_map.get(&line.para_start).unwrap_or(&line.y);
                visual_line_clips.push(qt_text_node::VisualLineClipInfo {
                    cache_idx: line.cache_slot,
                    qtextline_idx: line.qtextline_idx,
                    y: line.y,
                    height: line.height,
                    x: line.x,
                    width: line.width,
                    para_y,
                    doc_width: snapshot.width,
                });
            }

            // Issue #853: 静态 exclusion 直接来自统一 owner table。
            // 不再通过 StaticLinePatch 中间结构换算。
            // Issue #736 评论 5786231506: 静态层开始裁剪之前先检查本帧动画所需的
            // snapshot texture。缺纹理的 snapshot 对应 clip 不进入 static text clip，
            // canonical 正文直接显示。不能等静态层挖完以后到了 render_text_animation_layer()
            // 才 continue。overlay 可画 -> static 被接管；overlay 不可画 -> static 同帧
            // 恢复 canonical。两边是一条原子规则。
            // Issue #853: 只让资源已准备好的动画 owner 接管 target cluster。
            let available_clip_rects = if animation_resources_ready {
                plan.ownership.clips_for_static_rebuild().to_vec()
            } else {
                Vec::new()
            };
            let available_clip_rects = qt_text_node::merge_static_clip_rects(available_clip_rects);
            let clip_rects = &available_clip_rects;

            // Issue #658 评论 5620035970 问题 4: 正文从 padding 开始画，
            // origin_x = snapshot.padding，与 VisualLine.x = padding + x_off 一致。
            // Issue #658 评论 5620035970 问题 2: 传 snapshot.layout_generation，
            // 用 (generation, cache_slot) 查找 layout，与动画/IME 路径互不干扰。
            // Issue #668: 接住 rebuild 的成功/失败结果，透传给 qquickitem_impl。
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
            // 没有与本帧一致的 prepared layout 时不能提交新的 owner 状态。
            static_rebuild_ok = false;
        }
    } else {
        // 纯滚动帧 或 动画 progress 变化帧：只更新 QSGTransformNode 位移矩阵，
        // 不重建静态节点。不出现 clear()、QTextLayout、createLine()、addTextLayout()。
        // Issue #714 评论 5740007764: 动画期间静态层保持上一帧（事务开始时重建的）
        // 裁剪后节点不动，避免每帧销毁/重建静态 QSGTextNode 产生闪烁。
        qt_text_node::update_scroll_transform(root_raw, static_text.scroll_y);
    }

    // Layer 1: 文字动画层（保留：吐字/吞字/重排动画的纹理切片）
    // Issue #709 评论 issue-body-709: 动画层单独画 InsertReveal / DeleteConceal / ReflowMove，
    // 不把静态正文重新补回去。静态层在 clip_count > 0 时已通过 complement 区间
    // 把被动画接管的区域从静态正文里挖掉，动画层只负责画那些被接管的区域。
    // 两层在文档区域上互斥，避免静态正文盖住吐字/吞字动画。
    // Issue #715: 传 scroll_y 给动画层，让 AnimationLayerNode 设置和静态层一样的
    // translate(0, -scroll_y) 矩阵。动画 glyph 继续保留文档坐标，不在 Rust 侧逐个减 scroll_y。
    // 静态 rebuild 失败时保留前一帧的 static + animation 组合，不更新一半新状态。
    // 正常终点、取消和文档切换都走同一原子交接：只有静态层 rebuild 成功后，
    // 才在本次 update_paint_node 中更新/清空动画层。失败时保留旧 static + animation。
    if static_rebuild_ok {
        let glyphs = if animation_resources_ready && !plan.ownership.handoff_pending {
            plan.ownership.animated_glyphs.as_slice()
        } else {
            &[]
        };
        render_text_animation_layer(
            root_raw,
            item_ptr,
            glyphs,
            texture_cache,
            static_text.scroll_y,
        );
    }
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

    static_rebuild_ok
}

fn render_text_animation_layer(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    glyphs: &[super::render_plan::TextAnimationGlyphInfo],
    texture_cache: &TextureCache,
    scroll_y: f64,
) {
    // Issue #658 评论 5630650436: 空动画帧也进入 update_animation_layer，让 AnimationLayerNode
    // 自己把 node 和 texture 一次清干净。不要在这里提前 return，否则
    // update_animation_layer() 根本不调用，activeSet sweep 根本没运行，
    // AnimationLayerNode::m_texture_cache 里的 QSGTexture 继续留着。
    let mut glyph_data: Vec<f64> = Vec::new();
    let mut glyph_images: Vec<qmetaobject::QImage> = Vec::new();
    let mut source_rects: Vec<f64> = Vec::new();
    let mut snapshot_ids: Vec<u64> = Vec::new();

    for glyph in glyphs {
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
    }

    let glyph_count = glyph_data.len() / 5;
    if glyph_count > 0 && glyph_count == glyph_images.len() {
        let glyph_data_ptr = glyph_data.as_ptr();
        let image_ptrs: Vec<*const qmetaobject::QImage> = glyph_images
            .iter()
            .map(|img| img as *const qmetaobject::QImage)
            .collect();
        let image_ptrs_ptr = image_ptrs.as_ptr();
        let source_rects_ptr = source_rects.as_ptr();
        let snapshot_ids_ptr = snapshot_ids.as_ptr();

        scene_graph::update_animation_layer(
            root_raw,
            item_ptr,
            glyph_count as i32,
            glyph_data_ptr,
            image_ptrs_ptr,
            source_rects_ptr,
            snapshot_ids_ptr,
            scroll_y,
        );
    } else {
        scene_graph::clear_animation_layer(root_raw, item_ptr);
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
