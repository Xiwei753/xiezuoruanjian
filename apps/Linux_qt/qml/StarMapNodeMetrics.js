// =============================================================================
// StarMapNodeMetrics.js — 星图普通节点的统一度量（Issue #984）
// =============================================================================
//
// 层级：Linux_qt UI 层（纯度量函数，无状态、无 Core 调用）
// 职责：给 StarMapGraphController / StarMapActiveScene / StarMapSceneContent
//   提供同一份普通节点卡片宽高口径。普通节点是「名称 + 正文」的文字卡片，
//   不是旧版固定尺寸的标题条；所有几何（绘制、命中、连线端点、子图概览）
//   都必须调用这里，禁止各 QML 再写一套固定值。
//
// 约束：
//   - Core 只保存 position（节点左上角）与 title/content，宽高纯属平台显示层；
//   - 节点宽度固定，高度由名称 + 正文行数决定；
//   - 显示态正文最多预览 PREVIEW_MAX_LINES 行，editing === true 时按完整
//     正文行数计高，保证编辑时完整可见；
//   - 正文原样参与度量：识别 "\n" 与连续空行，不 trim、不合并空白；
//   - 行数按字符宽度确定性估算（CJK/全角 = 1 em，ASCII/半角 = 0.5 em）。
// =============================================================================

.pragma library

// ── 卡片口径（world px）──
var CARD_WIDTH = 220
var MIN_HEIGHT = 64
var PADDING = 12
var TITLE_FONT_SIZE = 14
var BODY_FONT_SIZE = 12
var TITLE_LINE_HEIGHT = 20
var BODY_LINE_HEIGHT = 17
var TITLE_MAX_LINES = 2
var PREVIEW_MAX_LINES = 5
var TITLE_BODY_GAP = 6

// 默认节点尺寸：新建节点 / 空白处找位置时使用，仍由同一套函数算出。
function defaultNodeWidth() {
    return CARD_WIDTH
}

function defaultNodeHeight() {
    return nodeHeight({ title: "", content: { type: "empty" } }, false)
}

// 兼容两种入参：graphData 节点（含 title/content）或裸 StarMapNodeContentDto。
// 传裸正文文本时按 inline content 处理，方便只量正文的调用方。
function _contentOf(nodeOrContent) {
    if (!nodeOrContent)
        return { type: "empty" }
    if (typeof nodeOrContent === "string")
        return { type: "inline", body: nodeOrContent }
    if (nodeOrContent.content !== undefined && nodeOrContent.content !== null)
        return nodeOrContent.content
    return nodeOrContent
}

function _titleOf(nodeOrContent) {
    if (nodeOrContent && nodeOrContent.title !== undefined
            && nodeOrContent.title !== null)
        return String(nodeOrContent.title)
    return ""
}

function contentTypeOf(nodeOrContent) {
    var c = _contentOf(nodeOrContent)
    return (c && c.type) ? String(c.type) : "empty"
}

function summaryOf(nodeOrContent) {
    var c = _contentOf(nodeOrContent)
    if (!c || c.summary === undefined)
        return null
    return c.summary
}

function bodyOf(nodeOrContent) {
    // Issue #984：正文预览 / 编辑只针对 empty / inline；引用类型即使携带
    // 同名可选字段，也不能当成普通正文显示或回写。
    if (!canEditBody(nodeOrContent))
        return ""
    var c = _contentOf(nodeOrContent)
    if (!c || c.body === undefined || c.body === null)
        return ""
    return String(c.body)
}

// Issue #984：正文只在 empty / inline 两种内容类型下可编辑；
// chapterRef / entityRef / externalRef 只改名称，不把引用内容改写成 inline。
function canEditBody(nodeOrContent) {
    var t = contentTypeOf(nodeOrContent)
    return t === "empty" || t === "inline"
}

function nodeWidth(nodeOrContent) {
    // 固定卡片宽度；保留参数是为了让三个 QML 用同一种调用形状。
    return CARD_WIDTH
}

function _contentWidth() {
    return CARD_WIDTH - PADDING * 2
}

// 全角字符区间（CJK / 韩文 / 全角标点 / 兼容汉字等）。
function _isFullWidth(code) {
    return (code >= 0x1100 && code <= 0x115F)
            || (code >= 0x2E80 && code <= 0x303E)
            || (code >= 0x3041 && code <= 0x33FF)
            || (code >= 0x3400 && code <= 0x4DBF)
            || (code >= 0x4E00 && code <= 0x9FFF)
            || (code >= 0xA000 && code <= 0xA4CF)
            || (code >= 0xAC00 && code <= 0xD7A3)
            || (code >= 0xF900 && code <= 0xFAFF)
            || (code >= 0xFE30 && code <= 0xFE6F)
            || (code >= 0xFF00 && code <= 0xFF60)
            || (code >= 0xFFE0 && code <= 0xFFE6)
            || (code >= 0x20000 && code <= 0x3FFFD)
}

// 单个码点的宽度（em）：全角 1，半角 0.5；tab 按 2（4 个半角空格）计。
function _charWidth(code) {
    if (code === 9)
        return 2.0
    if (code < 32)
        return 0.0
    return _isFullWidth(code) ? 1.0 : 0.5
}

function textWidth(text, fontSize) {
    var s = String(text === null || text === undefined ? "" : text)
    var width = 0
    for (var i = 0; i < s.length; i++) {
        var code = s.charCodeAt(i)
        var cp = code
        // 代理对：按一个码点计宽，避免把扩展区汉字算成两个半角。
        if (code >= 0xD800 && code <= 0xDBFF && i + 1 < s.length) {
            var low = s.charCodeAt(i + 1)
            if (low >= 0xDC00 && low <= 0xDFFF) {
                cp = (code - 0xD800) * 0x400 + (low - 0xDC00) + 0x10000
                i++
            }
        }
        width += _charWidth(cp) * fontSize
    }
    return width
}

// 按段落 + 字符宽度估算折行行数。
// "\n" 保留为一个硬换行；连续空行各占一行；"\r\n" 先归一成 "\n" 再量。
function wrappedLineCount(text, fontSize, maxWidth) {
    if (text === null || text === undefined)
        return 0
    var s = String(text).replace(/\r\n/g, "\n").replace(/\r/g, "\n")
    if (s.length === 0)
        return 1
    var paragraphs = s.split("\n")
    var total = 0
    for (var p = 0; p < paragraphs.length; p++) {
        var lineWidth = 0
        var lines = 1
        var para = paragraphs[p]
        for (var i = 0; i < para.length; i++) {
            var code = para.charCodeAt(i)
            var cp = code
            var step = 1
            if (code >= 0xD800 && code <= 0xDBFF && i + 1 < para.length) {
                var low = para.charCodeAt(i + 1)
                if (low >= 0xDC00 && low <= 0xDFFF) {
                    cp = (code - 0xD800) * 0x400 + (low - 0xDC00) + 0x10000
                    step = 2
                }
            }
            var w = _charWidth(cp) * fontSize
            if (lineWidth > 0 && lineWidth + w > maxWidth) {
                lines++
                lineWidth = w
            } else {
                lineWidth += w
            }
            i += step - 1
        }
        total += lines
    }
    return total
}

// 完整正文行数（编辑态 / 诊断用）。
function bodyLineCount(nodeOrContent) {
    var body = bodyOf(nodeOrContent)
    if (body.length === 0)
        return 0
    return wrappedLineCount(body, BODY_FONT_SIZE, _contentWidth())
}

// 显示态正文预览行数（最多 PREVIEW_MAX_LINES 行）。
function previewLineCount(nodeOrContent) {
    return Math.min(bodyLineCount(nodeOrContent), PREVIEW_MAX_LINES)
}

// 名称行数：显示态最多 TITLE_MAX_LINES 行；编辑态是单行 TextInput，按 1 行。
function titleLineCount(nodeOrContent, editing) {
    if (editing === true)
        return 1
    var lines = wrappedLineCount(_titleOf(nodeOrContent), TITLE_FONT_SIZE, _contentWidth())
    return Math.max(1, Math.min(lines, TITLE_MAX_LINES))
}

// 卡片高度（world px）。
// content 参数兼容 graphData 节点（含 title/content）或裸 content DTO；
// editing === true 时正文按完整行数展开，否则只按预览行数计高。
function nodeHeight(nodeOrContent, editing) {
    var titleLines = titleLineCount(nodeOrContent, editing === true)
    var bodyLines = editing === true
            ? bodyLineCount(nodeOrContent)
            : previewLineCount(nodeOrContent)
    var h = PADDING * 2 + titleLines * TITLE_LINE_HEIGHT
    if (bodyLines > 0)
        h += TITLE_BODY_GAP + bodyLines * BODY_LINE_HEIGHT
    return Math.max(MIN_HEIGHT, Math.ceil(h))
}
