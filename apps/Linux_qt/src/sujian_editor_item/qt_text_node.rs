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

    /// 从已排好的 per-paragraph 数据更新静态正文 QSGTextNode。
    ///
    /// 为每个段落创建独立的 QTextLayout（与 EditorLayout 完全一致的
    /// indent/wrap 参数），通过 addTextLayout 喂给 QSGTextNode。
    /// 若存在动画裁剪区域，用 QSGClipNode 裁掉静态正文的重叠区域。
    void update_text_node_from_paragraphs(
        QSGNode* root_raw, QQuickItem* item_ptr,
        const char** text_ptrs, const int* text_lens,
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

        QColor textColor(color_q);

        // 逐段落创建 QTextLayout，与 EditorLayout 的 indent/wrap 完全一致。
        // 记录上一个段落文本，用于合并同一段落的连续 VisualLine。
        QString lastParaText;
        QTextLayout* currentLayout = nullptr;
        double currentLayoutY = 0.0;

        for (int i = 0; i < para_count; i++) {
            QString paraText = QString::fromUtf8(text_ptrs[i], text_lens[i]);
            double y = para_y_arr[i];
            double iw = indent_w_arr[i];
            double lww = line_wrap_w_arr[i];
            double dw = doc_width_arr[i];

            if (paraText != lastParaText) {
                // 提交上一个段落的 layout
                if (currentLayout) {
                    currentLayout->endLayout();
                    textNode->addTextLayout(QPointF(0, currentLayoutY), currentLayout);
                    delete currentLayout;
                }

                // 创建新段落的 QTextLayout — 与 editor_layout_lines() 一致的参数
                currentLayout = new QTextLayout(
                    paraText, QFont(font_family, static_cast<int>(font_size)));
                QTextOption option;
                option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
                currentLayout->setTextOption(option);
                currentLayout->beginLayout();
                currentLayoutY = y;
                lastParaText = paraText;
            }

            if (paraText.isEmpty()) {
                // 空段落：创建一个空行占据正确高度
                QTextLine line = currentLayout->createLine();
                line.setLineWidth(dw);
                line.setPosition(QPointF(0, y - currentLayoutY));
            } else {
                // 非空行：按 EditorLayout 的 wrap_w - indent 逻辑设置宽度和位置
                QTextLine line = currentLayout->createLine();
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

        // ── 动画裁剪：用 QSGClipNode 裁掉动画接管区域 ──
        // 动画覆盖的 item 坐标范围 = clip_y - scroll_y
        // QSGClipNode 的 clip rect 在自身坐标空间（即 staticLayer 内部），
        // 而 staticLayer 的矩阵 translate(0, -scroll_y) 作用于其子节点。
        // 因此 clip rect 使用文档坐标（与 TextLayout 坐标一致）。
        if (clip_count > 0) {
            double animTop = clip_y_arr[0];
            double animBottom = clip_y_arr[0] + clip_h_arr[0];
            for (int c = 1; c < clip_count; c++) {
                double t = clip_y_arr[c];
                double b = clip_y_arr[c] + clip_h_arr[c];
                if (t < animTop) animTop = t;
                if (b > animBottom) animBottom = b;
            }
            double animH = animBottom - animTop;
            if (animH > 0.0) {
                // 在 staticLayer 和 textNode 之间插入 QSGClipNode
                // Clip rect 用文档坐标：动画区域的 y 范围，水平全宽
                staticLayer->removeChildNode(textNode);
                auto *clipNode = new QSGClipNode;
                clipNode->setIsRectangular(true);
                clipNode->setClipRect(QRectF(0, animTop, 99999.0, animH));
                staticLayer->appendChildNode(clipNode);
                clipNode->appendChildNode(textNode);
            }
        }

        // 滚动位移：通过 QSGTransformNode 矩阵平移
        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);
    }
}}

/// 由 `EditorLayout::snapshot()` 提供的 per-paragraph 排版数据。
///
/// 每个 VisualLine 对应一个条目，相同 paragraph_text 的连续 VisualLine
/// 在 C++ 侧合并为一个 QTextLayout，首行缩进仅作用于段落首行。
pub(crate) struct ParagraphLineInfo {
    /// 段落文本（不含尾部 \n）
    pub paragraph_text: String,
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

/// 从已排好的 VisualLine 数据更新静态正文 QSGTextNode。
///
/// 消费 `EditorLayout::snapshot()` 的结果，不再自行创建第二套全文 QTextLayout。
/// 若有动画裁剪区域，用 QSGClipNode 裁掉重叠区域。
///
/// # Safety
/// `root_raw` 和 `item_ptr` 必须是有效的 Qt 场景图指针。
pub fn update_text_node_from_paragraphs(
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
        update_text_node_from_paragraphs(
            root_raw, item_ptr,
            text_ptrs_ptr, text_lens_ptr,
            para_y_ptr, indent_ws_ptr,
            line_wrap_ws_ptr, doc_widths_ptr,
            para_count,
            font_size, font_family_q,
            scroll_y, color_q,
            clip_y_ptr, clip_h_ptr, clip_count
        );
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
