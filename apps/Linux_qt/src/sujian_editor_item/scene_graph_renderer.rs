use super::qt_text_node;
use super::render_plan::RenderPlan;
use super::texture_cache::TextureCache;
use crate::editor::layout::LayoutSnapshot;
use crate::editor::scene_graph;

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
    // Issue #714 评论 5740007764: 静态层只在 needs_relayout=true 时重建一次，
    // 不再因 clip_rects 非空而在每个动画帧重建。needs_relayout 由
    // layout_dirty || scene_dirty 驱动，覆盖以下需要重建静态层的情形：
    //   - 正文/layout/颜色变化（layout_dirty=true，GUI 线程 request_static_repaint）
    //   - 活动事务集合变化（scene_dirty=true，事务开始/结束/cancel/rebase）
    // 动画 progress 变化帧（tick 返回 false，不动 scene_dirty）只更新
    // animation layer / caret，不重建静态 QSGTextNode，避免 100ms 动画期间
    // 每帧销毁/重建静态节点导致闪烁（吐字刚显出的字闪、Enter 后下面文字闪）。
    //
    // clip_rects 表达的裁剪区域在事务开始的那一帧（scene_dirty=true
    // →needs_relayout=true）应用一次，动画期间保持不动；事务结束后
    // （tick 返回 true→scene_dirty=true）再重建一次完整 canonical 正文。
    //
    // Issue #709 评论 issue-body-709: 主链 AnimatedSlice.static_hidden_document_rects -> clip_rects
    // 表达的是"静态正文不能画的区域"（被动画层接管的文档区域）。clip_count > 0 时
    // qt_text_node 不再创建完整正文节点，只按 complement 区间生成 clip+text 节点，
    // 静态层与动画层在文档区域上互斥，避免静态正文盖住吐字/吞字动画。
    if static_text.needs_relayout {
        // 正文/layout/颜色变化 或 活动事务集合变化：重建静态节点（含裁剪）
        if let Some(snapshot) = static_text.layout_snapshot {
            // Issue #658: 按段落分组 VisualLine，每个段落对应一个 cache_idx。
            // cache_idx 直接从 VisualLine.cache_slot 读取（由 layout 阶段按段落出现顺序
            // 分配的稳定 slot），不再自行计数。
            let mut paragraphs: Vec<qt_text_node::ParagraphLineInfo> = Vec::new();
            let mut last_para_start: Option<usize> = None;

            for line in &snapshot.lines {
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
                para_y_map.entry(line.para_start).or_insert(line.y);
            }
            for line in &snapshot.lines {
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

            // Issue #727 评论 5755858583 问题2: 直接从 plan.clip_rects 读取裁剪区域，
            // 不再通过 StaticLinePatch 中间结构换算。
            // Issue #736 评论 5786231506: 静态层开始裁剪之前先检查本帧动画所需的
            // snapshot texture。缺纹理的 snapshot 对应 clip 不进入 static text clip，
            // canonical 正文直接显示。不能等静态层挖完以后到了 render_text_animation_layer()
            // 才 continue。overlay 可画 -> static 被接管；overlay 不可画 -> static 同帧
            // 恢复 canonical。两边是一条原子规则。
            let available_clip_rects: Vec<qt_text_node::AnimationClipRect> = plan
                .clip_rects
                .iter()
                .filter(|cr| texture_cache.contains_line(&cr.snapshot_id))
                .cloned()
                .collect();
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
    render_text_animation_layer(
        root_raw,
        item_ptr,
        plan,
        texture_cache,
        static_text.scroll_y,
    );
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
    plan: &RenderPlan,
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

    for glyph in &plan.text_animation.glyphs {
        // Issue #736 评论 5778543593 修改1: 缺该 snapshot_id 的纹理时，这个
        // CaretDriven frame 不进入 glyph_data。缺纹理直接 continue 跳过整个
        // glyph，不再塞 1×1 空图把"DeleteConceal 已生成但纹理没拿到"伪装成
        // "动画层正常执行只是什么也看不到"。有纹理时才 push 四个 vec，这样
        // 四个 vec 长度始终一致，glyph_count == glyph_images.len() 自然通过。
        let texture = match texture_cache.get_line(&glyph.snapshot_id) {
            Some(texture) => texture,
            None => continue,
        };

        glyph_data.extend_from_slice(&[glyph.x, glyph.y, glyph.w, glyph.h, glyph.opacity]);

        source_rects.extend_from_slice(&[
            glyph.source_rect.x,
            glyph.source_rect.y,
            glyph.source_rect.w,
            glyph.source_rect.h,
        ]);

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
