//! Qt 6.7+ 公开 API QSGTextNode 封装模块。
//!
//! 把自研写作区的静态正文渲染从"整块 QImage -> QSGImageNode"收口到
//! Qt 6.7+ 公开 API `QQuickWindow::createTextNode()` / `QSGTextNode::addTextLayout()`。
//!
//! ## 架构
//!
//! 场景图 child[0] 是 QSGTransformNode（静态正文层），其下挂一个 QSGTextNode。
//! - 正文变更/字体/宽度变化时：从 `EditorLayout::snapshot()` 已排好的 VisualLine
//!   数据构造 per-paragraph QTextLayout，调用 `QSGTextNode::addTextLayout()`，
//!   把 glyph 数据烘焙到节点内部。
//! - 滚动时：只改 QSGTransformNode 的位移矩阵（translate(0, -scroll_y)），
//!   不重新排版，不重新理解文字。
//!
//! ## 线程约束
//!
//! 使用 threaded render loop 时，`QQuickItem::updatePaintNode()` 在 render thread
//! 执行，GUI 线程此时被阻塞。QSGTextNode/QSGNode 的创建和修改留在
//! `updatePaintNode()` 的 render-thread 阶段。正文排版由唯一的 EditorLayout
//! 路线准备，Scene Graph 只消费已准备的数据，不因迁移到 QSGTextNode 又把
//! 排版职责塞回渲染阶段。
//!
//! ## 不变量
//!
//! - 不使用私有头文件 QSGInternalTextNode，只用公开 QSGTextNode。
//! - 不把 C++ 调用散进各文件，都集中在这个模块。
//! - 正文排版只由 EditorLayout（layout.rs）产生唯一 canonical 结果，
//!   本模块只消费已排好的 VisualLine，不再自行创建第二套全文 QTextLayout。
//! - QTextLayout 由 layout 阶段（editor_prepare_paragraph_visual_snapshot）创建，
//!   本模块通过 g_paragraph_layout_cache 读取已排好的 layout，不再 createLine。

use cpp::cpp;

cpp! {{
    #include <QtQuick/QSGTextNode>
    #include <QtQuick/QSGClipNode>
    #include <QtQuick/QQuickWindow>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <QtGui/QFont>
    #include <QtGui/QColor>
    #include <QtGui/QMatrix4x4>
    #include <QtCore/QPointF>
    #include <QDebug>
    #include <vector>
    #include <algorithm>

    // Issue #658: 段落布局缓存 — 由 layout.rs 的 cpp! 块定义，
    // 此处通过 extern 引用同一链接单元中的定义。
    extern void clear_paragraph_layout_cache();
    extern std::vector<QTextLayout*> g_paragraph_layout_cache;

    /// Issue #658: 从已排好的 per-paragraph 数据重建静态正文 QSGTextNode。
    ///
    /// 通过 cache_idx 在 g_paragraph_layout_cache 中查找已排好的 QTextLayout。
    /// 每个段落的 QTextLayout 在 editor_layout_lines() 阶段创建并缓存。
    void rebuild_text_node_from_paragraphs(
        QSGNode* root_raw, QQuickItem* item_ptr,
        const char** text_ptrs, const int* text_lens,
        const int* para_start_arr, const int* cache_idx_arr,
        const double* para_y_arr, const double* indent_w_arr,
        const double* doc_width_arr,
        int para_count,
        float font_size, const QString& font_family,
        double scroll_y, const QString& color_q,
        const double* clip_x_arr, const double* clip_y_arr,
        const double* clip_w_arr, const double* clip_h_arr,
        int clip_count
    ) {
        if (!root_raw || !item_ptr) return;
        QQuickWindow *window = item_ptr->window();
        if (!window) return;

        if (root_raw->childCount() == 0) return;
        auto *staticLayer = static_cast<QSGTransformNode*>(root_raw->childAtIndex(0));
        if (!staticLayer) return;

        // 获取或创建 QSGTextNode
        QSGTextNode *textNode = nullptr;
        if (staticLayer->childCount() > 0) {
            textNode = dynamic_cast<QSGTextNode*>(staticLayer->childAtIndex(0));
        }
        if (!textNode) {
            while (staticLayer->childCount() > 0) {
                QSGNode *old = staticLayer->childAtIndex(0);
                staticLayer->removeChildNode(old);
                delete old;
            }
            textNode = window->createTextNode();
            if (textNode) {
                staticLayer->appendChildNode(textNode);
            }
        }
        if (!textNode) return;

        textNode->clear();

        QColor textColor(color_q);

        // Issue #658: setColor() 必须在第一次 addTextLayout() 之前设置。
        // Qt 的 QSGTextNode 要求影响文本节点的属性在 addTextLayout() 前设置，
        // 否则加入的文字不保证使用后设的颜色。
        textNode->setColor(textColor);

        // Issue #658: 按段落遍历，通过 cache_idx 在 g_paragraph_layout_cache 中
        // 查找已排好的 QTextLayout，不再按视觉行索引查找。
        for (int i = 0; i < para_count; i++) {
            int cacheIdx = cache_idx_arr[i];
            double y = para_y_arr[i];

            if (cacheIdx >= 0 && cacheIdx < (int)g_paragraph_layout_cache.size()) {
                QTextLayout* cachedLayout = g_paragraph_layout_cache[cacheIdx];
                if (cachedLayout) {
                    // QSGTextNode::addTextLayout() 读取 QTextLayout 并不修改它，
                    // 可安全传递同一指针。
                    textNode->addTextLayout(QPointF(0, y), cachedLayout);
                }
            }
        }

        textNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);

        // Issue #658: 动画裁剪 — 按视觉行使用精确文档 x/y/w/h 裁剪。
        // 不再按 Y 整条裁剪（会丢掉 x/w 信息，导致同行其他文字消失）。
        if (clip_count > 0) {
            struct ClipRect { double x, y, w, h; };
            std::vector<ClipRect> clipRects;
            for (int c = 0; c < clip_count; c++) {
                double cx = clip_x_arr[c];
                double cy = clip_y_arr[c];
                double cw = clip_w_arr[c];
                double ch = clip_h_arr[c];
                if (ch > 0.0 && cw > 0.0) {
                    clipRects.push_back({cx, cy, cw, ch});
                }
            }
            if (!clipRects.empty()) {
                staticLayer->removeChildNode(textNode);

                for (int i = 0; i < para_count; i++) {
                    int cacheIdx = cache_idx_arr[i];
                    double py = para_y_arr[i];
                    double dw = doc_width_arr[i];
                    double nextY = (i + 1 < para_count) ? para_y_arr[i + 1] : py + 30.0;
                    double ph = nextY - py;
                    if (ph < 1.0) ph = 30.0;

                    struct XRange { double left, right; };
                    std::vector<XRange> mergedClips;

                    struct TempXClip { double x, x2; };
                    std::vector<TempXClip> xClips;
                    for (const auto& cr : clipRects) {
                        if (cr.y < py + ph && cr.y + cr.h > py) {
                            double crLeft = cr.x;
                            double crRight = cr.x + cr.w;
                            if (crLeft < 0) crLeft = 0;
                            if (crRight > dw) crRight = dw;
                            if (crLeft < crRight) {
                                xClips.push_back({crLeft, crRight});
                            }
                        }
                    }

                    std::sort(xClips.begin(), xClips.end(),
                        [](const TempXClip& a, const TempXClip& b) { return a.x < b.x; });
                    for (const auto& xc : xClips) {
                        if (!mergedClips.empty() && xc.x <= mergedClips.back().right) {
                            mergedClips.back().right = std::max(mergedClips.back().right, xc.x2);
                        } else {
                            mergedClips.push_back({xc.x, xc.x2});
                        }
                    }

                    std::vector<XRange> complements;
                    double curX = 0.0;
                    for (const auto& mc : mergedClips) {
                        if (mc.left > curX) {
                            complements.push_back({curX, mc.left});
                        }
                        curX = mc.right;
                    }
                    if (curX < dw) {
                        complements.push_back({curX, dw});
                    }

                    for (const auto& comp : complements) {
                        double clipW = comp.right - comp.left;
                        if (clipW < 0.5) continue;

                        auto *clipNode = new QSGClipNode;
                        clipNode->setIsRectangular(true);
                        clipNode->setClipRect(QRectF(comp.left, py, clipW, ph));
                        staticLayer->appendChildNode(clipNode);

                        QSGTextNode* clipTextNode = window->createTextNode();
                        if (clipTextNode) {
                            clipTextNode->setColor(textColor);
                            clipTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                            clipNode->appendChildNode(clipTextNode);

                            if (cacheIdx >= 0 && cacheIdx < (int)g_paragraph_layout_cache.size()) {
                                QTextLayout* cachedLayout = g_paragraph_layout_cache[cacheIdx];
                                if (cachedLayout) {
                                    clipTextNode->addTextLayout(QPointF(0, py), cachedLayout);
                                }
                            }
                            clipTextNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                        }
                    }
                }
            }
        }

        // 滚动位移：通过 QSGTransformNode 矩阵平移
        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);
    }

    /// 滚动帧：只更新 QSGTransformNode 的位移矩阵，不重建静态正文节点。
    ///
    /// 不出现 clear()、QTextLayout、createLine()、addTextLayout()。
    void update_scroll_transform(
        QSGNode* root_raw, double scroll_y
    ) {
        if (!root_raw || root_raw->childCount() == 0) return;
        auto *staticLayer = static_cast<QSGTransformNode*>(root_raw->childAtIndex(0));
        if (!staticLayer) return;

        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);
    }
}}

/// 由 `EditorLayout::snapshot()` 提供的 per-paragraph 排版数据。
///
/// Issue #658: 改为按段落传入数据（不再按视觉行）。cache_idx 是
/// g_paragraph_layout_cache 中的索引，用于查找已排好的 QTextLayout。
pub(crate) struct ParagraphLineInfo {
    /// 段落文本（不含尾部 \n）
    pub paragraph_text: String,
    /// 段落在文档中的字节偏移（段落身份标识）
    pub para_start: usize,
    /// 该段落在 g_paragraph_layout_cache 中的索引
    pub cache_idx: usize,
    /// 段落第一行在文档中的 y 坐标（文档坐标，即 VisualLine.y）
    pub y: f64,
    /// 段落第一行的缩进宽度
    pub indent_w: f64,
    /// 字号
    pub font_size: f32,
    /// 字体族
    pub font_family: String,
    /// 段落换行宽度（用于 C++ complement 区间判断）
    pub doc_width: f64,
}

/// 动画接管区域的裁剪矩形（文档坐标 x/y/w/h）。
///
/// Issue #658: 改为完整的 x/y/w/h 文档坐标矩形，
/// 由 build_render_plan_full() 通过 PreparedLineSnapshot::source_rect_to_document_rect() 转换。
pub(crate) struct AnimationClipRect {
    /// 文档 x 坐标
    pub x: f64,
    /// 文档 y 坐标
    pub y: f64,
    /// 宽度
    pub w: f64,
    /// 高度
    pub h: f64,
}

/// Issue #658: 从已排好的 per-paragraph 数据重建静态正文 QSGTextNode。
///
/// 通过 cache_idx 在 g_paragraph_layout_cache 中查找已排好的 QTextLayout。
/// animation_clip_rects 使用 x/y/w/h 文档坐标。
///
/// # Safety
/// `root_raw` 和 `item_ptr` 必须是有效的 Qt 场景图指针。
pub fn rebuild_text_node_from_paragraphs(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    paragraphs: &[ParagraphLineInfo],
    scroll_y: f64,
    color: &str,
    animation_clip_rects: &[AnimationClipRect],
) {
    if paragraphs.is_empty() {
        return;
    }

    let font_size = paragraphs[0].font_size;
    let font_family = &paragraphs[0].font_family;

    let font_family_q: qmetaobject::QString = font_family.clone().into();
    let color_q: qmetaobject::QString = color.to_string().into();

    let text_ptrs: Vec<*const u8> = paragraphs.iter().map(|p| p.paragraph_text.as_ptr()).collect();
    let text_lens: Vec<i32> = paragraphs.iter().map(|p| p.paragraph_text.len() as i32).collect();
    let para_starts: Vec<i32> = paragraphs.iter().map(|p| p.para_start as i32).collect();
    let cache_idxs: Vec<i32> = paragraphs.iter().map(|p| p.cache_idx as i32).collect();
    let para_y: Vec<f64> = paragraphs.iter().map(|p| p.y).collect();
    let indent_ws: Vec<f64> = paragraphs.iter().map(|p| p.indent_w).collect();
    let doc_widths: Vec<f64> = paragraphs.iter().map(|p| p.doc_width).collect();

    let clip_x: Vec<f64> = animation_clip_rects.iter().map(|c| c.x).collect();
    let clip_y: Vec<f64> = animation_clip_rects.iter().map(|c| c.y).collect();
    let clip_w: Vec<f64> = animation_clip_rects.iter().map(|c| c.w).collect();
    let clip_h: Vec<f64> = animation_clip_rects.iter().map(|c| c.h).collect();

    let para_count = paragraphs.len() as i32;
    let clip_count = animation_clip_rects.len() as i32;

    let text_ptrs_ptr = text_ptrs.as_ptr();
    let text_lens_ptr = text_lens.as_ptr();
    let para_starts_ptr = para_starts.as_ptr();
    let cache_idxs_ptr = cache_idxs.as_ptr();
    let para_y_ptr = para_y.as_ptr();
    let indent_ws_ptr = indent_ws.as_ptr();
    let doc_widths_ptr = doc_widths.as_ptr();
    let clip_x_ptr = clip_x.as_ptr();
    let clip_y_ptr = clip_y.as_ptr();
    let clip_w_ptr = clip_w.as_ptr();
    let clip_h_ptr = clip_h.as_ptr();

    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        text_ptrs_ptr as "const char**",
        text_lens_ptr as "const int*",
        para_starts_ptr as "const int*",
        cache_idxs_ptr as "const int*",
        para_y_ptr as "const double*",
        indent_ws_ptr as "const double*",
        doc_widths_ptr as "const double*",
        para_count as "int",
        font_size as "float",
        font_family_q as "QString",
        scroll_y as "double",
        color_q as "QString",
        clip_x_ptr as "const double*",
        clip_y_ptr as "const double*",
        clip_w_ptr as "const double*",
        clip_h_ptr as "const double*",
        clip_count as "int"
    ] {
        rebuild_text_node_from_paragraphs(
            root_raw, item_ptr,
            text_ptrs_ptr, text_lens_ptr,
            para_starts_ptr, cache_idxs_ptr,
            para_y_ptr, indent_ws_ptr,
            doc_widths_ptr,
            para_count,
            font_size, font_family_q,
            scroll_y, color_q,
            clip_x_ptr, clip_y_ptr,
            clip_w_ptr, clip_h_ptr,
            clip_count
        );
    });

    // Issue #658: 不在 rebuild 末尾清除 g_paragraph_layout_cache。
    // 布局缓存由 editor_layout_lines() 填充，持久存在供后续帧读取；
    // 仅在 prepare_document_visual_snapshot() 开头和 editor_layout_lines() 开头清除。
}

/// 滚动帧：只更新静态正文层的 QSGTransformNode 位移矩阵。
///
/// 不出现 clear()、QTextLayout、createLine()、addTextLayout()。
/// 在 `updatePaintNode()` 中 needs_relayout=false 时调用。
///
/// # Safety
/// `root_raw` 必须是有效的 Qt 场景图指针。
pub fn update_scroll_transform(root_raw: *mut std::ffi::c_void, scroll_y: f64) {
    if root_raw.is_null() {
        return;
    }
    cpp!(unsafe [
        root_raw as "QSGNode*",
        scroll_y as "double"
    ] {
        update_scroll_transform(root_raw, scroll_y);
    });
}

/// 清除静态正文 QSGTextNode 的内容。
///
/// 在文本清空或项目销毁时调用，移除 QSGTextNode 内所有 glyph 几何。
///
/// # Safety
/// `root_raw` 必须是有效的 Qt 场景图指针。
#[allow(dead_code)]
pub fn clear_static_text_node(root_raw: *mut std::ffi::c_void) {
    // SAFETY: root_raw 来自 Qt 场景图，在 updatePaintNode（render thread）调用。
    cpp!(unsafe [root_raw as "QSGNode*"] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root || root->childCount() == 0) return;

        auto *staticLayer = static_cast<QSGTransformNode*>(root->childAtIndex(0));
        if (!staticLayer || staticLayer->childCount() == 0) return;

        auto *textNode = dynamic_cast<QSGTextNode*>(staticLayer->childAtIndex(0));
        if (textNode) {
            textNode->clear();
            textNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }
    });
}
