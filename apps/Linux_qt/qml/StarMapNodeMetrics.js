// =============================================================================
// StarMapNodeMetrics.js — 星图普通节点的统一度量（Issue #984）
// =============================================================================
//
// 层级：Linux_qt UI 层（纯几何口径 + 图级布局快照缓存）
// 职责：给 StarMapGraphController / StarMapActiveScene / StarMapSceneContent
//   提供同一份普通节点卡片宽高。普通节点是「名称 + 正文」的文字卡片，
//   不是旧版固定尺寸的标题条；绘制、命中、连线端点、子图概览必须共用
//   同一份由图数据加载时算好的布局快照。
//
// 约束：
//   - Core 只保存 position（节点左上角）与 title/content，宽高纯属平台显示层；
//   - 高度不是字符宽度估算，也不是“活动画布实测、未打开子图按 64 猜”：
//     由 StarMapNodeMeasurer 用 Qt 的真实文本排版（Text.forceLayout +
//     contentHeight）在数据加载时测量整张图，写入 snapshotFor() 缓存；
//   - 快照身份 = starmapId + 全部节点的标题/内容签名；内容变化后重新测量；
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
// 编辑浮层的最大高度（屏幕 px）；实际尺寸还会被视口边界收窄。
var EDIT_MAX_HEIGHT = 420

// 图级布局快照：starmapId -> { signature, sizes: { nodeId: {width,height} } }。
// 快照只在图数据/内容变化时重建，供活动画布、命中、边端点、Embed 概览共用。
var _snapshots = {}

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
// chapterRef / entityRef 只改名称，不把引用内容改写成 inline。
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

// 节点内容签名：标题、内容类型、摘要、正文都会影响排版高度。
function nodeSignature(node) {
    var content = _contentOf(node)
    return String(node && node.title !== undefined && node.title !== null ? node.title : "未命名")
            + "\u001f" + contentTypeOf(node)
            + "\u001f" + String(summaryOf(node) !== null ? summaryOf(node) : "")
            + "\u001f" + bodyOf(node)
}

function _graphSignature(nodes) {
    var parts = []
    for (var i = 0; i < nodes.length; i++)
        parts.push(_nodeId(nodes[i]) + "\u001e" + nodeSignature(nodes[i]))
    return parts.join("\u001d")
}

// 取得/构建一张图的节点尺寸快照。measureNode(node) 必须返回
// { width, height }（由平台真实文本排版算出，world px）。
// 同一 starmapId 下内容签名不变时复用；变化时整图重测，保证所有消费方
// 拿到的都是同一份、包含未打开子图的完整几何。
function snapshotFor(starmapId, nodes, measureNode) {
    var list = nodes || []
    var key = starmapId !== undefined && starmapId !== null ? String(starmapId) : ""
    var signature = _graphSignature(list)
    var cached = _snapshots[key]
    if (cached && cached.signature === signature)
        return cached.sizes

    var sizes = {}
    if (measureNode) {
        for (var i = 0; i < list.length; i++) {
            var node = list[i]
            var id = _nodeId(node)
            if (id === "")
                continue
            var size = measureNode(node)
            if (size && isFinite(size.width) && isFinite(size.height)) {
                sizes[id] = {
                    width: Math.max(1, Number(size.width)),
                    height: Math.max(MIN_HEIGHT, Number(size.height))
                }
            }
        }
        _snapshots[key] = { signature: signature, sizes: sizes }
    }
    return sizes
}

// 只读取已构建的快照；未构建返回 null。几何消费方不允许在缺快照时
// 用另一套固定高度继续算，必须先通过 snapshotFor() 走平台文本测量。
function snapshotSizes(starmapId) {
    var key = starmapId !== undefined && starmapId !== null ? String(starmapId) : ""
    var cached = _snapshots[key]
    return cached ? cached.sizes : null
}

function sizeFor(sizes, node) {
    var id = _nodeId(node)
    var size = sizes ? sizes[id] : null
    if (size)
        return size
    return { width: defaultNodeWidth(), height: defaultNodeHeight() }
}

function invalidateSnapshot(starmapId) {
    var key = starmapId !== undefined && starmapId !== null ? String(starmapId) : ""
    delete _snapshots[key]
}

function clearAllSnapshots() {
    _snapshots = {}
}
