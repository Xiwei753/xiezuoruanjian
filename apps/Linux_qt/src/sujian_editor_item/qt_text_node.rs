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

    /// 从已排好的 per-paragraph 数据重建静态正文 QSGTextNode。
    ///
    /// 与 EditorLayout 完全一致的 indent/wrap 参数，使用 setPixelSize()
    /// 保证字号和换行几何与 layout.rs 一致。段落身份用 para_start 区分，
    /// 不用文本内容比较。动画裁剪用 complement geometry：显示动画区域之外的静态正文。
    void rebuild_text_node_from_paragraphs(
        QSGNode* root_raw, QQuickItem* item_ptr,
        const char** text_ptrs, const int* text_lens,
        const int* para_start_arr,
        const double* para_y_arr, const double* indent_w_arr,
        const double* line_wrap_w_arr, const double* doc_width_arr,
        int para_count,
        float font_size, const QString& font_family,
        double scroll_y, const QString& color_q,
        const double* clip_y_arr, const double* clip_h_arr, int clip_count
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

        // 与 layout.rs 完全一致的字体构造：使用 setPixelSize()，
        // 而非 QFont(family, int) 的 pointSize 构造函数。
        QFont font(font_family);
        font.setPixelSize(static_cast<int>(font_size));

        QColor textColor(color_q);

        // 段落身份用 para_start（字节偏移），不用文本内容比较。
        // 段落合并：连续 VisualLine 的 para_start 相同则属同一段落。
        int lastParaStart = -1;
        QTextLayout* currentLayout = nullptr;
        double currentLayoutY = 0.0;

        for (int i = 0; i < para_count; i++) {
            QString paraText = QString::fromUtf8(text_ptrs[i], text_lens[i]);
            int paraStart = para_start_arr[i];
            double y = para_y_arr[i];
            double iw = indent_w_arr[i];
            double lww = line_wrap_w_arr[i];
            double dw = doc_width_arr[i];

            if (paraStart != lastParaStart) {
                // 提交上一个段落的 layout
                if (currentLayout) {
                    currentLayout->endLayout();
                    textNode->addTextLayout(QPointF(0, currentLayoutY), currentLayout);
                    delete currentLayout;
                    currentLayout = nullptr;
                }

                lastParaStart = paraStart;
                currentLayoutY = y;

                // 空段落不创建 QTextLayout，只保留几何占位
                if (!paraText.isEmpty()) {
                    currentLayout = new QTextLayout(paraText, font);
                    QTextOption option;
                    option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
                    currentLayout->setTextOption(option);
                    currentLayout->beginLayout();
                }
            }

            // 任何路径都不能解引用空的 currentLayout
            if (!currentLayout) {
                // 空段落占位：跳过，几何由后续行的 y 偏移隐式保证
                continue;
            }

            QTextLine line = currentLayout->createLine();
            if (paraText.isEmpty()) {
                line.setLineWidth(dw);
                line.setPosition(QPointF(0, y - currentLayoutY));
            } else {
                line.setLineWidth(lww);
                line.setPosition(QPointF(iw, y - currentLayoutY));
            }
        }

        // 提交最后一个段落
        if (currentLayout) {
            currentLayout->endLayout();
            textNode->addTextLayout(QPointF(0, currentLayoutY), currentLayout);
            delete currentLayout;
        }

        textNode->setColor(textColor);
        textNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);

        // ── 动画裁剪：complement geometry ──
        // QSGClipNode 只保留子树在 clipRect 内的部分。
        // 我们要显示动画区域之外的静态正文，所以 clipRect 应该是
        // 文档区域减去动画区域后的可见区域。
        if (clip_count > 0) {
            // 先按 y 排序裁剪区间
            struct ClipRange { double y, b; };
            std::vector<ClipRange> ranges;
            for (int c = 0; c < clip_count; c++) {
                double cy = clip_y_arr[c];
                double ch = clip_h_arr[c];
                if (ch > 0.0) {
                    ranges.push_back({cy, cy + ch});
                }
            }
            std::sort(ranges.begin(), ranges.end(),
                [](const ClipRange& a, const ClipRange& b) { return a.y < b.y; });

            // 合并重叠区间
            std::vector<ClipRange> merged;
            for (const auto& r : ranges) {
                if (!merged.empty() && r.y <= merged.back().b) {
                    merged.back().b = std::max(merged.back().b, r.b);
                } else {
                    merged.push_back(r);
                }
            }

            if (!merged.empty()) {
                // 移除现有子节点（textNode）
                staticLayer->removeChildNode(textNode);

                // 计算 complement 区间：文档可见区域减去动画区域
                // 文档区域假设从 0 到 textNode 渲染范围内的任意值。
                // 用极大值表示"到文档末尾"。
                double docTop = 0.0;
                double docBottom = 1e9;

                // 第一段：[docTop, merged[0].y)
                // 中间段：[merged[i].b, merged[i+1].y)
                // 最后段：[merged.back().b, docBottom)

                bool hasComplement = false;
                if (merged[0].y > docTop) {
                    hasComplement = true;
                }
                for (size_t i = 1; i < merged.size(); i++) {
                    if (merged[i].y > merged[i-1].b) {
                        hasComplement = true;
                        break;
                    }
                }
                if (merged.back().b < docBottom) {
                    hasComplement = true;
                }

                if (!hasComplement) {
                    // 整个文档被动画覆盖，不需要显示静态正文
                    // （动画层会接管）
                } else {
                    // 为每个 complement 区间创建 QSGClipNode + 副本 textNode
                    // 第一个区间用原始 textNode，后续区间创建新的
                    QSGTextNode* firstTextNode = textNode;
                    bool firstUsed = false;

                    if (merged[0].y > docTop) {
                        auto *clipNode = new QSGClipNode;
                        clipNode->setIsRectangular(true);
                        clipNode->setClipRect(QRectF(0, docTop, 99999.0, merged[0].y - docTop));
                        staticLayer->appendChildNode(clipNode);
                        clipNode->appendChildNode(firstTextNode);
                        firstUsed = true;
                    }

                    for (size_t i = 1; i < merged.size(); i++) {
                        if (merged[i].y > merged[i-1].b) {
                            auto *clipNode = new QSGClipNode;
                            clipNode->setIsRectangular(true);
                            clipNode->setClipRect(QRectF(0, merged[i-1].b, 99999.0, merged[i].y - merged[i-1].b));
                            staticLayer->appendChildNode(clipNode);
                            // 为每个 complement 区间创建独立的 textNode 副本
                            QSGTextNode* copyNode = window->createTextNode();
                            if (copyNode) {
                                copyNode->setColor(textColor);
                                copyNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                                clipNode->appendChildNode(copyNode);
                            }
                        }
                    }

                    if (merged.back().b < docBottom) {
                        auto *clipNode = new QSGClipNode;
                        clipNode->setIsRectangular(true);
                        double h = docBottom - merged.back().b;
                        if (h > 1e8) h = 99999.0;
                        clipNode->setClipRect(QRectF(0, merged.back().b, 99999.0, h));
                        staticLayer->appendChildNode(clipNode);
                        if (!firstUsed) {
                            clipNode->appendChildNode(firstTextNode);
                        } else {
                            QSGTextNode* copyNode = window->createTextNode();
                            if (copyNode) {
                                copyNode->setColor(textColor);
                                copyNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
                                clipNode->appendChildNode(copyNode);
                            }
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
/// 每个 VisualLine 对应一个条目，相同 para_start 的连续 VisualLine
/// 在 C++ 侧合并为一个 QTextLayout，首行缩进仅作用于段落首行。
pub(crate) struct ParagraphLineInfo {
    /// 段落文本（不含尾部 \n）
    pub paragraph_text: String,
    /// 段落在文档中的字节偏移（段落身份标识）
    pub para_start: usize,
    /// 该行在段落中的 y 坐标（文档坐标，即 VisualLine.y）
    pub y: f64,
    /// 段落第一行的缩进宽度（仅首行有值，续行为 0）
    pub indent_w: f64,
    /// 该行的换行宽度（首行为 wrap_w - indent，续行为 wrap_w）
    pub line_wrap_w: f64,
    /// 字号
    pub font_size: f32,
    /// 字体族
    pub font_family: String,
    /// 文档宽度（用于首行换行宽度的基准）
    pub doc_width: f64,
}

/// 动画接管区域的裁剪矩形（文档坐标 y）。
///
/// 当动画层接管某些区域时，静态正文需要裁掉这些区域以避免双绘。
pub(crate) struct AnimationClipRect {
    /// 文档 y 坐标（与 VisualLine.y 一致）
    pub y: f64,
    /// 高度
    pub h: f64,
}

/// 从已排好的 VisualLine 数据重建静态正文 QSGTextNode。
///
/// 消费 `EditorLayout::snapshot()` 的结果，不再自行创建第二套全文 QTextLayout。
/// 若有动画裁剪区域，用 complement geometry 裁掉动画区域内的静态正文，
/// 保留动画区域外的静态正文。
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
    let para_y: Vec<f64> = paragraphs.iter().map(|p| p.y).collect();
    let indent_ws: Vec<f64> = paragraphs.iter().map(|p| p.indent_w).collect();
    let line_wrap_ws: Vec<f64> = paragraphs.iter().map(|p| p.line_wrap_w).collect();
    let doc_widths: Vec<f64> = paragraphs.iter().map(|p| p.doc_width).collect();

    let clip_y: Vec<f64> = animation_clip_rects.iter().map(|c| c.y).collect();
    let clip_h: Vec<f64> = animation_clip_rects.iter().map(|c| c.h).collect();

    let para_count = paragraphs.len() as i32;
    let clip_count = animation_clip_rects.len() as i32;

    let text_ptrs_ptr = text_ptrs.as_ptr();
    let text_lens_ptr = text_lens.as_ptr();
    let para_starts_ptr = para_starts.as_ptr();
    let para_y_ptr = para_y.as_ptr();
    let indent_ws_ptr = indent_ws.as_ptr();
    let line_wrap_ws_ptr = line_wrap_ws.as_ptr();
    let doc_widths_ptr = doc_widths.as_ptr();
    let clip_y_ptr = clip_y.as_ptr();
    let clip_h_ptr = clip_h.as_ptr();

    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        text_ptrs_ptr as "const char**",
        text_lens_ptr as "const int*",
        para_starts_ptr as "const int*",
        para_y_ptr as "const double*",
        indent_ws_ptr as "const double*",
        line_wrap_ws_ptr as "const double*",
        doc_widths_ptr as "const double*",
        para_count as "int",
        font_size as "float",
        font_family_q as "QString",
        scroll_y as "double",
        color_q as "QString",
        clip_y_ptr as "const double*",
        clip_h_ptr as "const double*",
        clip_count as "int"
    ] {
        rebuild_text_node_from_paragraphs(
            root_raw, item_ptr,
            text_ptrs_ptr, text_lens_ptr,
            para_starts_ptr,
            para_y_ptr, indent_ws_ptr,
            line_wrap_ws_ptr, doc_widths_ptr,
            para_count,
            font_size, font_family_q,
            scroll_y, color_q,
            clip_y_ptr, clip_h_ptr, clip_count
        );
    });
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
