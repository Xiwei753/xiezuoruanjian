// =============================================================================
// StarMapNodeMetrics.js — 星图普通节点的统一度量（Issue #984）
// =============================================================================
//
// 层级：Linux_qt UI 层（纯度量函数 + 当前界面的实测高度缓存）
// 职责：给 StarMapGraphController / StarMapActiveScene / StarMapSceneContent
//   提供同一份普通节点卡片宽高口径。普通节点是「名称 + 正文」的文字卡片，
//   不是旧版固定尺寸的标题条；所有几何（绘制、命中、连线端点、子图概览）
//   都必须调用这里，禁止各 QML 再写一套固定值。
//
// 约束：
//   - Core 只保存 position（节点左上角）与 title/content，宽高纯属平台显示层；
//   - 节点宽度固定，高度来自当前平台 Text 的实际排版结果：
//     StarMapActiveScene 的显示层把 Text.contentHeight 报回来，
//     recordMeasuredHeight() 写入本模块缓存；控制器 / 命中 / 边端点 /
//     概览都读同一份缓存。
//   - 未测量前只回退 MIN_HEIGHT，不再按字符宽度猜行数；不使用第二套固定高度。
//   - 正文原样参与显示：识别 "\n" 与连续空行，不 trim、不合并空白。
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
// 编辑层在世界坐标里的最大高度；视口更小时由 ActiveScene 再收窄。
var EDIT_MAX_HEIGHT = 420

// 当前界面里由 QML Text 实测出的卡片高度（world px），按节点 id 缓存。
var _measuredHeights = {}

function defaultNodeWidth() {
    return CARD_WIDTH
}

function defaultNodeHeight() {
    return MIN_HEIGHT
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

function _nodeId(nodeOrContent) {
    if (!nodeOrContent || typeof nodeOrContent !== "object")
        return ""
    return nodeOrContent.id !== undefined && nodeOrContent.id !== null
            ? String(nodeOrContent.id) : ""
}

// QML 显示层实测出的高度（已含 padding / 标题 / 预览正文）。
// 返回 true 表示高度确实变化，调用方需要重算边与命中。
function recordMeasuredHeight(nodeOrContent, height) {
    var id = _nodeId(nodeOrContent)
    var value = Number(height)
    if (id === "" || !isFinite(value) || value <= 0)
        return false
    value = Math.max(MIN_HEIGHT, Math.ceil(value))
    var previous = _measuredHeights[id]
    if (previous !== undefined && Math.abs(previous - value) < 0.75)
        return false
    _measuredHeights[id] = value
    return true
}

function measuredHeight(nodeOrContent) {
    var id = _nodeId(nodeOrContent)
    var value = id !== "" ? _measuredHeights[id] : undefined
    return value !== undefined ? value : MIN_HEIGHT
}

function clearMeasuredHeight(nodeId) {
    if (nodeId === undefined || nodeId === null)
        return
    delete _measuredHeights[String(nodeId)]
}

function clearAllMeasuredHeights() {
    _measuredHeights = {}
}

// 卡片高度（world px）。显示态读 QML Text 实测缓存；
// 编辑态不再把完整正文撑进世界几何（编辑层由 ActiveScene 限高 + 滚动）。
function nodeHeight(nodeOrContent, editing) {
    if (editing === true)
        return Math.max(MIN_HEIGHT, Math.min(EDIT_MAX_HEIGHT, measuredHeight(nodeOrContent)))
    return measuredHeight(nodeOrContent)
}
