//! Qt 6.7+ 公开 API QSGTextNode 封装模块。
//!
//! 把自研写作区的静态正文渲染从"整块 QImage -> QSGImageNode"收口到
//! Qt 6.7+ 公开 API `QQuickWindow::createTextNode()` / `QSGTextNode::addTextLayout()`。
//!
//! ## 架构
//!
//! 场景图 child[0] 是 QSGTransformNode（静态正文层），其下挂一个 QSGTextNode。
//! - 正文变更/字体/宽度变化时：重新创建 QTextLayout 排版，调用
//!   `QSGTextNode::clear()` + `QSGTextNode::addTextLayout(&layout)`，把 glyph 数据
//!   烘焙到节点内部。
//! - 滚动时：只改 QSGTransformNode 的位移矩阵（translate(0, -scroll_y)），
//!   不重新排版，不重新理解文字。
//!
//! ## 线程约束
//!
//! QSGTextNode 只能在 QSG 渲染线程（updatePaintNode）中创建和更新。
//! QTextLayout 只能在 GUI 线程使用；由于 updatePaintNode 在 GUI 线程执行，
//! 两者在同一线程，安全。
//!
//! ## 不变量
//!
//! - 不使用私有头文件 QSGInternalTextNode，只用公开 QSGTextNode。
//! - 不把 C++ 调用散进各文件，都集中在这个模块。

use cpp::cpp;
use qmetaobject::QString;

cpp! {{
    #include <QtQuick/QSGTextNode>
    #include <QtQuick/QQuickWindow>
    #include <QtGui/QTextLayout>
    #include <QtGui/QTextOption>
    #include <QtGui/QFont>
    #include <QtGui/QColor>
    #include <QtGui/QMatrix4x4>
    #include <QtCore/QPointF>
    #include <QDebug>
}}

/// 更新静态正文 QSGTextNode。
///
/// 在场景图 child[0]（QSGTransformNode）下获取或创建 QSGTextNode，
/// 根据 `needs_relayout` 决定是否重新排版并调用 `addTextLayout`。
/// 滚动通过 QSGTransformNode 的位移矩阵实现，不重新排版。
///
/// # 参数
/// - `root_raw`：场景图根节点（QSGTransformNode*）
/// - `item_ptr`：QQuickItem 指针，用于获取 QQuickWindow
/// - `text`：正文 UTF-8 文本
/// - `font_size`/`font_family`：字体参数
/// - `width`/`padding`/`line_spacing`/`text_indent`：布局参数
/// - `scroll_y`：滚动偏移（文档逻辑坐标）
/// - `color`：文字颜色（CSS 字符串，如 "#E2E2E5"）
/// - `needs_relayout`：是否需要重新排版（正文/字体/宽度变更时为 true）
///
/// # Safety
/// `root_raw` 和 `item_ptr` 必须是有效的 Qt 场景图指针，在 GUI/QSG 线程调用。
pub fn update_static_text_node(
    root_raw: *mut std::ffi::c_void,
    item_ptr: *mut std::ffi::c_void,
    text: &str,
    font_size: f32,
    font_family: &str,
    width: f64,
    padding: f64,
    line_spacing: f64,
    text_indent: f64,
    scroll_y: f64,
    color: &str,
    needs_relayout: bool,
) {
    let text_bytes = text.as_bytes();
    let text_ptr = text_bytes.as_ptr();
    let text_len = text_bytes.len();
    let family: QString = font_family.into();
    let color_q: QString = color.into();

    // SAFETY: root_raw/item_ptr 来自 Qt 场景图，在 updatePaintNode（GUI 线程）调用；
    // text_ptr/text_len 来自 Rust &str，在函数执行期间有效；family/color_q 是
    // qmetaobject::QString，cpp! 宏保证其 C++ 侧生命周期覆盖本次调用。
    cpp!(unsafe [
        root_raw as "QSGNode*",
        item_ptr as "QQuickItem*",
        text_ptr as "const char*",
        text_len as "size_t",
        font_size as "float",
        family as "QString",
        width as "double",
        padding as "double",
        line_spacing as "double",
        text_indent as "double",
        scroll_y as "double",
        color_q as "QString",
        needs_relayout as "bool"
    ] {
        auto *root = static_cast<QSGTransformNode*>(root_raw);
        if (!root || !item_ptr) return;

        QQuickWindow *window = item_ptr->window();
        if (!window) return;

        // child[0] 是 QSGTransformNode（静态正文层）
        if (root->childCount() == 0) return;
        auto *staticLayer = static_cast<QSGTransformNode*>(root->childAtIndex(0));
        if (!staticLayer) return;

        // 获取或创建 QSGTextNode
        QSGTextNode *textNode = nullptr;
        if (staticLayer->childCount() > 0) {
            textNode = dynamic_cast<QSGTextNode*>(staticLayer->childAtIndex(0));
        }
        if (!textNode) {
            // 移除旧的非 QSGTextNode 子节点
            while (staticLayer->childCount() > 0) {
                QSGNode *old = staticLayer->childAtIndex(0);
                staticLayer->removeChildNode(old);
                delete old;
            }
            // createTextNode() 无参数，返回后手动 append 到 staticLayer
            textNode = window->createTextNode();
            if (textNode) {
                staticLayer->appendChildNode(textNode);
            }
        }

        if (!textNode) return;

        // 重新排版：正文/字体/宽度变更时
        if (needs_relayout) {
            textNode->clear();

            QString qtext = QString::fromUtf8(text_ptr, static_cast<int>(text_len));
            QFont font(family);
            font.setPixelSize(static_cast<int>(font_size));

            QTextLayout layout(qtext, font);
            QTextOption option;
            option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
            layout.setTextOption(option);

            double available = (width - padding * 2.0 - text_indent);
            if (available < font_size) available = font_size;

            layout.beginLayout();
            double y = padding;
            forever {
                QTextLine line = layout.createLine();
                if (!line.isValid()) break;
                line.setLineWidth(static_cast<qreal>(available));
                line.setPosition(QPointF(padding + text_indent, static_cast<qreal>(y)));
                y += static_cast<double>(line.height()) * line_spacing;
            }
            layout.endLayout();

            QColor textColor(color_q);
            textNode->setColor(textColor);
            // position=(0,0)：滚动由 QSGTransformNode 矩阵处理
            textNode->addTextLayout(QPointF(0, 0), &layout);
            textNode->markDirty(QSGNode::DirtyGeometry | QSGNode::DirtyMaterial);
        }

        // 滚动位移：通过 QSGTransformNode 矩阵平移，不重新排版
        QMatrix4x4 mat;
        mat.translate(0.0f, static_cast<float>(-scroll_y), 0.0f);
        staticLayer->setMatrix(mat);
        staticLayer->markDirty(QSGNode::DirtyMatrix);
    });
}

/// 清除静态正文 QSGTextNode 的内容。
///
/// 在文本清空或项目销毁时调用，移除 QSGTextNode 内所有 glyph 几何。
///
/// # Safety
/// `root_raw` 必须是有效的 Qt 场景图指针，在 GUI/QSG 线程调用。
#[allow(dead_code)]
pub fn clear_static_text_node(root_raw: *mut std::ffi::c_void) {
    // SAFETY: root_raw 来自 Qt 场景图，在 updatePaintNode（GUI 线程）调用。
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
