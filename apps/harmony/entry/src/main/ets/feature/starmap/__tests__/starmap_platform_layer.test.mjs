// starmap_platform_layer.test.mjs — 星图平台层（#789 评论 5868714244）纯逻辑测试。
//
// 纯 JS（.mjs），不依赖 ArkUI / Core bridge，Node 直接运行：
//   node apps/harmony/entry/src/main/ets/feature/starmap/__tests__/starmap_platform_layer.test.mjs
//
// 本评论要求的行为契约（对应实现文件）：
//   1. freeform 布局直接用 node.position，position 是唯一真相；
//      加载页面不许重新排网格 —— buildFreeformLayout (StarMapLayout.ets)
//   2. Embed 位置只读 embed.position，宽高/圆角由平台层定义；
//      zIndex 在普通节点之上 —— buildEmbedLayoutNodes (StarMapLayout.ets)
//   3. 自动布局只在用户显式点击时调用，且保留 nodeId/尺寸；
//      结果通过 position patch 写回 —— autoGridLayoutNodes / generatePositionPatches
//   4. 节点/Embed 拖动：屏幕位移 ÷ zoomScale 落到布局坐标，返回新数组
//      —— moveLayoutNode（StarMapScreen.moveNodeBy / moveEmbedBy）
//   5. 缩放返回父层：缩到 MIN_ZOOM 之下且存在父星图 → 返回父星图；
//      没有父星图则夹在 MIN_ZOOM —— zoomOut / applyPinchScale
//   6. 关系写入 JSON 形状：edge id/时间戳平台端生成，target detail 全字段显式
//      null（Core 侧 serde 要求键存在）—— NativeStarMapBridge.addStarMapEdge
//
// 被测规格与对应 .ets 内联实现严格一致。

// ── 常量（与 StarMapLayout.ets 一致）──
const DEFAULT_NODE_WIDTH = 160
const DEFAULT_NODE_HEIGHT = 80
const DEFAULT_NODE_RADIUS = 16
const DEFAULT_NODE_ZINDEX = 0
const DEFAULT_EMBED_WIDTH = 200
const DEFAULT_EMBED_HEIGHT = 120
const DEFAULT_EMBED_RADIUS = 12
const GRID_HORIZONTAL_SPACING = 200
const GRID_VERTICAL_SPACING = 120

// ── 常量（与 StarMapScreen.ets 一致）──
const MIN_ZOOM = 0.3
const MAX_ZOOM = 3
const ZOOM_STEP = 0.1

// ── 被测规格：buildFreeformLayout ──
function buildFreeformLayout(nodes) {
  const result = []
  for (const node of nodes) {
    result.push({
      nodeId: node.id,
      x: node.position.x,
      y: node.position.y,
      width: DEFAULT_NODE_WIDTH,
      height: DEFAULT_NODE_HEIGHT,
      radius: DEFAULT_NODE_RADIUS,
      zIndex: DEFAULT_NODE_ZINDEX,
      collapsed: false
    })
  }
  return result
}

// ── 被测规格：buildEmbedLayoutNodes ──
function buildEmbedLayoutNodes(embeds) {
  const result = []
  for (const embed of embeds) {
    result.push({
      nodeId: embed.instanceId,
      x: embed.position.x,
      y: embed.position.y,
      width: DEFAULT_EMBED_WIDTH,
      height: DEFAULT_EMBED_HEIGHT,
      radius: DEFAULT_EMBED_RADIUS,
      zIndex: DEFAULT_NODE_ZINDEX + 1,
      collapsed: false
    })
  }
  return result
}

// ── 被测规格：autoGridLayoutNodes ──
function autoGridLayoutNodes(nodes, canvasWidth, canvasHeight) {
  const nodeCount = nodes.length
  if (nodeCount <= 0) {
    return []
  }
  const cols = Math.ceil(Math.sqrt(nodeCount))
  const rows = Math.ceil(nodeCount / cols)
  const gridWidth = (cols - 1) * GRID_HORIZONTAL_SPACING + DEFAULT_NODE_WIDTH
  const gridHeight = (rows - 1) * GRID_VERTICAL_SPACING + DEFAULT_NODE_HEIGHT
  const offsetX = (canvasWidth - gridWidth) / 2
  const offsetY = (canvasHeight - gridHeight) / 2
  const result = []
  for (let i = 0; i < nodeCount; i++) {
    const col = i % cols
    const row = Math.floor(i / cols)
    result.push({
      nodeId: nodes[i].nodeId,
      x: offsetX + col * GRID_HORIZONTAL_SPACING,
      y: offsetY + row * GRID_VERTICAL_SPACING,
      width: nodes[i].width,
      height: nodes[i].height,
      radius: nodes[i].radius,
      zIndex: nodes[i].zIndex,
      collapsed: nodes[i].collapsed
    })
  }
  return result
}

// ── 被测规格：applyFreeformLayout / moveLayoutNode ──
function applyFreeformLayout(nodes, positions) {
  const result = []
  for (const node of nodes) {
    const pos = positions.get(node.nodeId)
    if (pos !== undefined) {
      result.push({
        nodeId: node.nodeId,
        x: pos.x,
        y: pos.y,
        width: node.width,
        height: node.height,
        radius: node.radius,
        zIndex: node.zIndex,
        collapsed: node.collapsed
      })
    } else {
      result.push(node)
    }
  }
  return result
}

function moveLayoutNode(layoutNodes, nodeId, screenDx, screenDy, zoomScale) {
  const ln = layoutNodes.find(n => n.nodeId === nodeId)
  if (!ln) {
    return layoutNodes
  }
  const positions = new Map()
  positions.set(nodeId, { x: ln.x + screenDx / zoomScale, y: ln.y + screenDy / zoomScale })
  return applyFreeformLayout(layoutNodes, positions)
}

// ── 被测规格：generatePositionPatches ──
function generatePositionPatches(layoutNodes) {
  const patches = []
  for (const node of layoutNodes) {
    patches.push({ nodeId: node.nodeId, x: node.x, y: node.y })
  }
  return patches
}

// ── 被测规格：缩放策略（StarMapScreen）──
function zoomOut(zoomScale, parentStarmapId) {
  if (zoomScale - ZOOM_STEP < MIN_ZOOM && parentStarmapId.length > 0) {
    return { zoomScale: zoomScale, returnedToParent: true }
  }
  return { zoomScale: Math.max(MIN_ZOOM, zoomScale - ZOOM_STEP), returnedToParent: false }
}

function zoomIn(zoomScale) {
  return Math.min(MAX_ZOOM, zoomScale + ZOOM_STEP)
}

function applyPinchScale(pinchBaseScale, gestureScale, parentStarmapId) {
  const scale = pinchBaseScale * gestureScale
  if (scale < MIN_ZOOM && parentStarmapId.length > 0) {
    return { zoomScale: null, returnedToParent: true }
  }
  return { zoomScale: Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, scale)), returnedToParent: false }
}

// ── 被测规格：关系写入 JSON（NativeStarMapBridge）──
function nodeTargetPath(starmapId, nodeId) {
  return {
    starmapId: starmapId,
    segments: [],
    target: {
      type: 'node',
      nodeId: nodeId,
      anchorId: null,
      projectId: null,
      volumeId: null,
      chapterId: null,
      rangeStart: null,
      rangeEnd: null,
      entityType: null,
      entityId: null,
      uri: null
    }
  }
}

function buildEdgeJson(starmapId, fromNodeId, toNodeId, kind, label, uuid, now) {
  return JSON.stringify({
    id: uuid,
    from: nodeTargetPath(starmapId, fromNodeId),
    to: nodeTargetPath(starmapId, toNodeId),
    kind: kind,
    label: label,
    payload: null,
    createdAt: now,
    updatedAt: now
  })
}

// ── 断言工具 ──
let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}
function eq(a, b) {
  return JSON.stringify(a) === JSON.stringify(b)
}
function find(nodes, id) {
  return nodes.find(n => n.nodeId === id)
}

console.log('1. freeform 直接用 node.position，位置不被重排')
{
  const graph = {
    nodes: [
      { id: 'n1', position: { x: 137.5, y: -42 } },
      { id: 'n2', position: { x: 0, y: 0 } }
    ]
  }
  const layout = buildFreeformLayout(graph.nodes)
  assert(layout.length === 2, '两个节点都进布局')
  assert(find(layout, 'n1').x === 137.5 && find(layout, 'n1').y === -42,
    'n1 的 authored position (137.5, -42) 原样进布局')
  assert(find(layout, 'n2').x === 0 && find(layout, 'n2').y === 0,
    'n2 的 (0, 0) 原样进布局（不被当成"未布局"覆盖）')
  assert(find(layout, 'n1').width === 160 && find(layout, 'n1').height === 80,
    '节点显示尺寸由平台层定义')
}

console.log('2. 加载路径不排网格：freeform 结果 ≠ 自动网格结果')
{
  const nodes = [
    { id: 'n1', position: { x: 137.5, y: -42 } },
    { id: 'n2', position: { x: 300, y: 90 } },
    { id: 'n3', position: { x: 410, y: 12 } }
  ]
  const freeform = buildFreeformLayout(nodes)
  const grid = autoGridLayoutNodes(freeform, 800, 600)
  assert(!eq(freeform, grid), 'freeform 布局与网格布局不同（加载不会改成网格）')
  assert(find(freeform, 'n1').x === 137.5, 'freeform 保持 authored x')
  assert(find(grid, 'n1').x !== 137.5 || find(grid, 'n1').y !== -42,
    '自动网格只在显式调用时重排（网格坐标由画布中心算出）')
}

console.log('3. 自动网格布局：保留 nodeId 与尺寸，居中排布')
{
  const nodes = buildFreeformLayout([
    { id: 'a', position: { x: 1, y: 2 } },
    { id: 'b', position: { x: 3, y: 4 } },
    { id: 'c', position: { x: 5, y: 6 } },
    { id: 'd', position: { x: 7, y: 8 } }
  ])
  const grid = autoGridLayoutNodes(nodes, 800, 600)
  assert(eq(grid.map(n => n.nodeId), ['a', 'b', 'c', 'd']), 'nodeId 顺序保留')
  assert(grid.every(n => n.width === 160 && n.height === 80 && n.radius === 16),
    '尺寸/圆角保留')
  // cols = ceil(sqrt(4)) = 2；gridWidth = 200 + 160 = 360 → offsetX = 220
  assert(grid[0].x === 220 && grid[1].x === 420, '第一行按 200 水平间距排布')
  // rows = 2；gridHeight = 120 + 80 = 200 → offsetY = 200
  assert(grid[0].y === 200 && grid[2].y === 320, '第二行按 120 垂直间距排布')
  assert(autoGridLayoutNodes([], 800, 600).length === 0, '空图返回空布局')
}

console.log('4. position patch 写回：nodeId + 新坐标')
{
  const layout = buildFreeformLayout([
    { id: 'n1', position: { x: 10, y: 20 } },
    { id: 'n2', position: { x: 30, y: 40 } }
  ])
  const patches = generatePositionPatches(layout)
  assert(eq(patches, [{ nodeId: 'n1', x: 10, y: 20 }, { nodeId: 'n2', x: 30, y: 40 }]),
    '自动布局/保存走同一份 position patch')
}

console.log('5. Embed 布局：只读 embed.position，平台层定义宽高')
{
  const embeds = [
    { instanceId: 'emb1', targetStarmapId: 'sm2', label: '支线', position: { x: 640, y: 48 } }
  ]
  const layout = buildEmbedLayoutNodes(embeds)
  assert(layout.length === 1, '嵌入进布局')
  assert(layout[0].nodeId === 'emb1', 'Embed 布局 id 用 instanceId')
  assert(layout[0].x === 640 && layout[0].y === 48, 'Embed 位置来自 embed.position')
  assert(layout[0].width === 200 && layout[0].height === 120 && layout[0].radius === 12,
    'Embed 宽高圆角由平台层定义')
  assert(layout[0].zIndex === 1, 'Embed 层级在普通节点之上')
}

console.log('6. 拖动：屏幕位移 ÷ zoomScale，返回新数组不改原数组')
{
  const layout = buildFreeformLayout([
    { id: 'n1', position: { x: 100, y: 100 } },
    { id: 'n2', position: { x: 200, y: 200 } }
  ])
  const moved = moveLayoutNode(layout, 'n1', 40, -30, 2)
  assert(find(moved, 'n1').x === 120 && find(moved, 'n1').y === 85,
    'n1 位移按 zoomScale=2 换算成画布坐标 (20, -15)')
  assert(find(layout, 'n1').x === 100 && find(layout, 'n1').y === 100,
    '原数组不被原地修改（返回新数组触发刷新）')
  assert(find(moved, 'n2').x === 200 && find(moved, 'n2').y === 200, '其他节点不动')
  const embeds = buildEmbedLayoutNodes([{ instanceId: 'emb1', position: { x: 640, y: 48 } }])
  const movedEmbed = moveLayoutNode(embeds, 'emb1', 30, 20, 1)
  assert(find(movedEmbed, 'emb1').x === 670 && find(movedEmbed, 'emb1').y === 68,
    'Embed 走同一条拖动路径（写入 embed.position）')
  assert(moveLayoutNode(layout, 'nope', 10, 10, 1) === layout, '未知 id 原样返回')
}

console.log('7. 缩放返回父层：缩到 MIN 之下回父星图，无父则夹住')
{
  assert(eq(zoomOut(0.5, 'parent'), { zoomScale: 0.4, returnedToParent: false }),
    '0.5 → 0.4 正常缩小')
  assert(eq(zoomOut(0.3, 'parent'), { zoomScale: 0.3, returnedToParent: true }),
    '已在 MIN(0.3) 再缩小 → 返回父星图，不改缩放')
  assert(eq(zoomOut(0.3, ''), { zoomScale: 0.3, returnedToParent: false }),
    '没有父星图时夹在 MIN，不误跳转')
  assert(zoomIn(2.95) === 3, '放大夹在 MAX')
  const pinched = applyPinchScale(1, 0.2, 'parent')
  assert(pinched.returnedToParent === true && pinched.zoomScale === null,
    '双指缩到 MIN 之下 → 返回父星图')
  const pinchedClamp = applyPinchScale(1, 0.2, '')
  assert(pinchedClamp.zoomScale === 0.3 && pinchedClamp.returnedToParent === false,
    '没有父星图时双指缩放夹在 MIN')
  assert(applyPinchScale(1, 1.2, '').zoomScale === 1.2, '双指放大按基准比例计算')
}

console.log('8. 关系写入 JSON：id/时间戳平台端生成，target detail 全字段存在')
{
  const json = buildEdgeJson('sm1', 'n1', 'n2', 'RelatedTo', null, 'uuid-1', 1700000000000)
  const edge = JSON.parse(json)
  assert(edge.id === 'uuid-1', 'edge id 由平台端生成（不是空串）')
  assert(edge.createdAt === 1700000000000 && edge.updatedAt === 1700000000000,
    '时间戳平台端写入')
  assert(edge.kind === 'RelatedTo' && edge.label === null && edge.payload === null,
    'kind/label/payload 按 StarMapEdgeDto 字段出现')
  const detailKeys = [
    'type', 'nodeId', 'anchorId', 'projectId', 'volumeId', 'chapterId',
    'rangeStart', 'rangeEnd', 'entityType', 'entityId', 'uri'
  ]
  for (const side of ['from', 'to']) {
    assert(edge[side].starmapId === 'sm1' && eq(edge[side].segments, []),
      `${side} 是 StarMapTargetPathDto（带 starmapId + segments）`)
    const missing = detailKeys.filter(k => !(k in edge[side].target))
    assert(missing.length === 0, `${side}.target 全字段存在（Core serde 需要键在）`)
  }
  assert(edge.from.target.nodeId === 'n1' && edge.to.target.nodeId === 'n2',
    'from/to 指向真实节点')
  assert(edge.from.target.type === 'node' && edge.from.target.anchorId === null,
    'type=node，未用字段显式 null')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
