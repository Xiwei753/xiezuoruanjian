// =============================================================================
// StarMapNodeMeasurer.qml — 星图普通节点的平台文本排版测量（Issue #984）
// =============================================================================
//
// 用隐藏的 Qt Quick Text（同一字体、同一宽度、同一换行/行高规则）真实排版，
// forceLayout() 后读取 contentHeight。活动画布与子图概览共用同一份结果，
// 不依赖节点已经显示在屏幕上，也不按字符宽度猜行数。
// =============================================================================

import QtQuick
import "StarMapNodeMetrics.js" as StarMapNodeMetrics

Item {
    id: root

    visible: false
    width: 0
    height: 0

    readonly property real textWidth:
        StarMapNodeMetrics.CARD_WIDTH - StarMapNodeMetrics.PADDING * 2

    // 返回 { width, height }（world px）。空正文只量标题；引用类型不显示正文。
    function measureNode(node) {
        var title = node && node.title !== undefined && node.title !== null
                && String(node.title).length > 0
                ? String(node.title) : qsTr("未命名")
        var titleHeight = measureText(title,
                                      StarMapNodeMetrics.TITLE_FONT_SIZE,
                                      StarMapNodeMetrics.TITLE_LINE_HEIGHT,
                                      StarMapNodeMetrics.TITLE_MAX_LINES,
                                      true)
        var body = StarMapNodeMetrics.bodyOf(node)
        var bodyHeight = body.length > 0
                ? measureText(body,
                              StarMapNodeMetrics.BODY_FONT_SIZE,
                              StarMapNodeMetrics.BODY_LINE_HEIGHT,
                              StarMapNodeMetrics.PREVIEW_MAX_LINES,
                              false)
                : 0
        var height = StarMapNodeMetrics.PADDING * 2 + titleHeight
                + (bodyHeight > 0
                   ? StarMapNodeMetrics.TITLE_BODY_GAP + bodyHeight : 0)
        return {
            width: StarMapNodeMetrics.CARD_WIDTH,
            height: Math.max(StarMapNodeMetrics.MIN_HEIGHT, Math.ceil(height))
        }
    }

    function measureText(value, fontSize, lineHeight, maxLines, bold) {
        probe.text = value
        probe.width = root.textWidth
        probe.font.pixelSize = fontSize
        probe.font.weight = bold ? Font.DemiBold : Font.Normal
        probe.lineHeight = lineHeight
        probe.maximumLineCount = maxLines
        probe.forceLayout()
        // maximumLineCount 之外的内容不参与卡片高度；行高是固定值，
        // 因而 contentHeight 与实际显示行数的边界一致。
        var measured = isFinite(probe.contentHeight) ? probe.contentHeight : 0
        return Math.max(lineHeight, Math.min(measured, lineHeight * maxLines))
    }

    Text {
        id: probe
        visible: false
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        lineHeightMode: Text.FixedHeight
        elide: Text.ElideRight
        maximumLineCount: StarMapNodeMetrics.TITLE_MAX_LINES
    }
}
