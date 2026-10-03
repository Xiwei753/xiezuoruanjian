// starmap_platform_layer.test.mjs — 星图平台层（#789 评论 5868714244）纯逻辑测试。
//
// 纯 JS（.mjs），不依赖 ArkUI / Core bridge，Node 直接运行：
//   node apps/harmony/entry/src/main/ets/feature/starmap/__tests__/starmap_platform_layer.test.mjs
//
// 本评论要求的行为契约（对应实现文件）：
//   1. freeform 布局直接用 node.position，position 是唯一真相；
//      加载页面不许重新排网格 —— buildFreeformLayout (StarMapLayout.ets)
//   2. Embed 位置只读 embed.position；Embed 是正圆，宽 = 高 = 直径，圆角 = 直径 / 2；
//      zIndex 在普通节点之上 —— buildEmbedLayoutNodes (StarMapLayout.ets)
//   2b. Embed 命中按圆判断：圆外不命中（含外接矩形的方形角），圆内再分 title /
//      圆环 border / innerContent；边端点落在真实圆周上而不是矩形边
//      —— hitTestWithScene / lineCircleIntersection (StarMapGeometry.ets, #813)
//   2c. title / 圆环的交互热区尺寸只有一份真相（StarMapGeometry 导出的
//      EMBED_TITLE_HIT_HEIGHT / EMBED_BORDER_HIT_WIDTH，UI 侧必须 import 同一份）；
//      圆形 responseRegion 按"最大带高"分带，丢弃的圆帽高度不随 zoom 放大
//      —— buildCircleResponseBands (StarMapGeometry.ets, #813)
//   3. 自动布局只在用户显式点击时调用，且保留 nodeId/尺寸；
//      结果通过 position patch 写回 —— autoGridLayoutNodes / generatePositionPatches
//   4. 节点/Embed 拖动：屏幕位移 ÷ zoomScale 落到布局坐标，返回新数组
//      —— moveLayoutNode（StarMapScreen.moveNodeBy / moveEmbedBy）
//   5. 缩放：工具栏按倍数步进、双指连续，边界只防数值事故；
//      不再有"缩到 MIN 之下自动回父星图"的跳转 —— 子星图由 Deep Zoom 档位
//      自然显隐（#821）—— zoomIn / zoomOut / applyPinchScale / clampCameraScale
//   6. 关系写入 JSON 形状：edge id/时间戳平台端生成，target detail 全字段显式
//      null（Core 侧 serde 要求键存在）—— NativeStarMapBridge.addStarMapEdge
//
// 被测规格与对应 .ets 内联实现严格一致。

// ── 常量（与 StarMapLayout.ets 一致）──
const DEFAULT_NODE_WIDTH = 160
const DEFAULT_NODE_HEIGHT = 80
const DEFAULT_NODE_RADIUS = 16
const DEFAULT_NODE_ZINDEX = 0
const DEFAULT_EMBED_DIAMETER = 200
const GRID_HORIZONTAL_SPACING = 200
const GRID_VERTICAL_SPACING = 120

// ── 常量（与 StarMapViewport.ets 一致，#821）──
const CAMERA_SCALE_MIN = 1e-4
const CAMERA_SCALE_MAX = 1e5
const ZOOM_FACTOR = 1.2

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
// #821：没有显示边界入参了。位置就是 authored position，尺寸恒为基准直径的正圆。
function buildEmbedLayoutNodes(embeds) {
  const result = []
  for (const embed of embeds) {
    result.push({
      nodeId: embed.instanceId,
      x: embed.position.x,
      y: embed.position.y,
      width: DEFAULT_EMBED_DIAMETER,
      height: DEFAULT_EMBED_DIAMETER,
      radius: DEFAULT_EMBED_DIAMETER / 2,
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

// ── 被测规格：缩放策略（StarMapScreen + StarMapViewport，#821）──
// 相机范围是数值安全边界，不是产品上限；工具栏用乘法步进，手感与当前档位无关。
function clampCameraScale(scale) {
  if (!Number.isFinite(scale) || scale <= 0) {
    return 1
  }
  return Math.min(CAMERA_SCALE_MAX, Math.max(CAMERA_SCALE_MIN, scale))
}

function zoomOut(zoomScale) {
  return clampCameraScale(zoomScale / ZOOM_FACTOR)
}

function zoomIn(zoomScale) {
  return clampCameraScale(zoomScale * ZOOM_FACTOR)
}

function applyPinchScale(pinchBaseScale, gestureScale) {
  return clampCameraScale(pinchBaseScale * gestureScale)
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

// ── 常量（与 StarMapGeometry.ets 一致）──
// Embed 命中规格：几何层唯一真相源，UI 层必须 import 同一份。
// EMBED_BORDER_HIT_WIDTH 是*交互热区*宽度（12vp），不是视觉描边宽度（1/3vp）。
const EMBED_TITLE_HIT_HEIGHT = 24
const EMBED_BORDER_HIT_WIDTH = 12

// ── 被测规格：pointInEmbedCircle (StarMapGeometry.ets) ──
function pointInEmbedCircle(rect, x, y) {
  const cx = rect.x + rect.width / 2
  const cy = rect.y + rect.height / 2
  const dx = x - cx
  const dy = y - cy
  return dx * dx + dy * dy <= (rect.width / 2) * (rect.width / 2)
}

// ── 被测规格：hitTestWithScene (StarMapGeometry.ets) ──
// Embed 是正圆：先做圆内判断，不在圆内跳过该 Embed（继续看更下层 rect）；
// 圆内再按顶部 title 条 / 最外侧圆环 border / 其余 innerContent 区分。
function embedHitMetricsForScene(sceneScale, embedRadius) {
  const scale = sceneScale > 0 ? sceneScale : 1
  const limit = embedRadius > 0 ? embedRadius : 0
  return {
    titleHitHeight: Math.min(limit, EMBED_TITLE_HIT_HEIGHT / scale),
    borderHitWidth: Math.min(limit, EMBED_BORDER_HIT_WIDTH / scale)
  }
}

function hitTestWithScene(rects, screenX, screenY, scenePath, embedInstanceIds, sceneScale) {
  for (let i = rects.length - 1; i >= 0; i--) {
    const r = rects[i]
    const isEmbed = embedInstanceIds.has(r.nodeId)
    if (isEmbed) {
      if (!pointInEmbedCircle(r, screenX, screenY)) continue
      const metrics = embedHitMetricsForScene(sceneScale, r.width / 2)
      if (screenY <= r.y + metrics.titleHitHeight) {
        return { scenePath, objectKind: 'embedTitle', objectId: r.nodeId, hitRegion: 'title' }
      }
      const cx = r.x + r.width / 2
      const cy = r.y + r.height / 2
      const inner = Math.max(0, r.width / 2 - metrics.borderHitWidth)
      const dist = Math.sqrt((screenX - cx) * (screenX - cx) + (screenY - cy) * (screenY - cy))
      if (dist >= inner) {
        return { scenePath, objectKind: 'embedBorder', objectId: r.nodeId, hitRegion: 'border' }
      }
      return { scenePath, objectKind: 'embedInnerContent', objectId: r.nodeId, hitRegion: 'innerContent' }
    }
    if (screenX >= r.x && screenX <= r.x + r.width &&
        screenY >= r.y && screenY <= r.y + r.height) {
      return { scenePath, objectKind: 'node', objectId: r.nodeId, hitRegion: 'body' }
    }
  }
  return null
}

// ── 被测规格：lineCircleIntersection (StarMapGeometry.ets) ──
function lineCircleIntersection(cx, cy, radius, tx, ty) {
  const dx = tx - cx
  const dy = ty - cy
  const distSq = dx * dx + dy * dy
  if (distSq === 0) return { x: cx, y: cy }
  const dist = Math.sqrt(distSq)
  const t = radius / dist
  return { x: cx + dx * t, y: cy + dy * t }
}

// ── 被测规格：lineRectIntersection (StarMapGeometry.ets) ──
function lineRectIntersection(cx, cy, tx, ty, rectX, rectY, rectW, rectH) {
  const dx = tx - cx
  const dy = ty - cy
  if (dx === 0 && dy === 0) return { x: cx, y: cy }
  let tMin = Infinity
  const tryT = (t, ok) => { if (t > 0 && ok) tMin = Math.min(tMin, t) }
  if (dx !== 0) {
    const t = (rectX - cx) / dx
    const iy = cy + t * dy
    tryT(t, iy >= rectY && iy <= rectY + rectH)
    const t2 = (rectX + rectW - cx) / dx
    const iy2 = cy + t2 * dy
    tryT(t2, iy2 >= rectY && iy2 <= rectY + rectH)
  }
  if (dy !== 0) {
    const t = (rectY - cy) / dy
    const ix = cx + t * dx
    tryT(t, ix >= rectX && ix <= rectX + rectW)
    const t2 = (rectY + rectH - cy) / dy
    const ix2 = cx + t2 * dx
    tryT(t2, ix2 >= rectX && ix2 <= rectX + rectW)
  }
  if (tMin === Infinity) return { x: cx, y: cy }
  return { x: cx + tMin * dx, y: cy + tMin * dy }
}

// ── 被测规格：edgeEndpointBoundaryPoint (StarMapGeometry.ets) ──
// 普通节点走矩形边界，Embed 走圆周边界，两者不能共用一套算法。
// 正式边渲染和拉线预览都必须走这里，否则预览会从中心出发再跳到边界。
function edgeEndpointBoundaryPoint(rect, isEmbed, tx, ty) {
  const cx = rect.x + rect.width / 2
  const cy = rect.y + rect.height / 2
  if (isEmbed) return lineCircleIntersection(cx, cy, rect.width / 2, tx, ty)
  return lineRectIntersection(cx, cy, tx, ty, rect.x, rect.y, rect.width, rect.height)
}

// ── 被测规格：computeEdgeRender 端点 (StarMapGeometry.ets) ──
function computeEdgeEndpoints(fromRect, toRect, fromIsEmbed, toIsEmbed) {
  const fromCx = fromRect.x + fromRect.width / 2
  const fromCy = fromRect.y + fromRect.height / 2
  const toCx = toRect.x + toRect.width / 2
  const toCy = toRect.y + toRect.height / 2
  return {
    start: edgeEndpointBoundaryPoint(fromRect, fromIsEmbed, toCx, toCy),
    end: edgeEndpointBoundaryPoint(toRect, toIsEmbed, fromCx, fromCy)
  }
}

// ── 被测规格：buildCircleResponseBands (StarMapGeometry.ets) ──
// ArkUI 组件默认热区是整个矩形，borderRadius + clip 只裁视觉不裁热区，
// 圆形子视图必须自己把热区切成若干水平带逼近圆。
//
// 每条带的半宽取上下两条边所在高度处圆半宽的较小值（内切），
// 保证永远不会把圆外的点算成圆内；极点处宽度为 0 的带直接丢弃。
// 带数按 ceil(直径 / 最大带高) 推导，不写死：写死会让丢弃的圆帽高度
// （= 带高）随 zoom 一起放大（3x、直径 600、32 带 → 上下各 18.75vp 点不到）。
const MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP = 3

function circleHalfWidthAtY(r, y) {
  const dy = y - r
  if (dy <= -r || dy >= r) return 0
  return Math.sqrt(r * r - dy * dy)
}

function buildCircleResponseBands(diameter, maxBandHeight = MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP) {
  const result = []
  if (diameter <= 0 || maxBandHeight <= 0) return result
  const r = diameter / 2
  const bands = Math.max(1, Math.ceil(diameter / maxBandHeight))
  const bandHeight = diameter / bands
  for (let i = 0; i < bands; i++) {
    const y = i * bandHeight
    const half = Math.min(circleHalfWidthAtY(r, y), circleHalfWidthAtY(r, y + bandHeight))
    if (half <= 0) continue
    result.push({ x: r - half, y: y, width: half * 2, height: bandHeight })
  }
  return result
}

// ── 被测规格：StarMapGestureStateTracker (StarMapGestureState.ets) ──
// 与 .ets 里的实现逐字段对齐：#816 之后归属按手势类型分开记（pinch/pan/connect/操作对象），
// 且 gestureTracker 只有一个实例，由根 Scene 持有、各 Scene 共享。
function copyPath(path) {
  return path.map((seg) => ({ type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId }))
}
function sameScenePath(a, b) {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (a[i].type !== b[i].type ||
        a[i].instanceId !== b[i].instanceId ||
        a[i].nodeId !== b[i].nodeId) return false
  }
  return true
}
function createGestureStateTracker() {
  function emptyState() {
    return {
      mode: 'idle', activeItemId: '', activeItemKind: 'node',
      ownerScenePath: [], activeItemScenePath: null,
      panOwnerScenePath: null,
      connectOwnerScenePath: null, connectSourceScenePath: null,
      startPoint: { x: 0, y: 0 }, currentPoint: { x: 0, y: 0 },
      targetItemId: '', targetItemScenePath: null
    }
  }
  let state = emptyState()
  return {
    beginPanCanvas(ownerScenePath, startX, startY) {
      const next = emptyState()
      next.mode = 'panCanvas'
      next.ownerScenePath = copyPath(ownerScenePath)
      next.panOwnerScenePath = copyPath(ownerScenePath)
      next.startPoint = { x: startX, y: startY }
      next.currentPoint = { x: startX, y: startY }
      state = next
    },
    beginNodeMenu(ownerScenePath, nodeId, startX, startY) {
      const next = emptyState()
      next.mode = 'nodeMenu'
      next.activeItemId = nodeId
      next.activeItemKind = 'node'
      next.ownerScenePath = copyPath(ownerScenePath)
      next.activeItemScenePath = copyPath(ownerScenePath)
      next.startPoint = { x: startX, y: startY }
      next.currentPoint = { x: startX, y: startY }
      state = next
    },
    beginConnect(ownerScenePath, nodeId, startX, startY) {
      const next = emptyState()
      next.mode = 'connect'
      next.activeItemId = nodeId
      next.activeItemKind = 'node'
      next.ownerScenePath = copyPath(ownerScenePath)
      next.activeItemScenePath = copyPath(ownerScenePath)
      next.connectOwnerScenePath = copyPath(ownerScenePath)
      next.connectSourceScenePath = copyPath(ownerScenePath)
      next.startPoint = { x: startX, y: startY }
      next.currentPoint = { x: startX, y: startY }
      state = next
    },
    beginMoveNode(ownerScenePath, nodeId, startX, startY) {
      const next = emptyState()
      next.mode = 'moveNode'
      next.activeItemId = nodeId
      next.activeItemKind = 'node'
      next.ownerScenePath = copyPath(ownerScenePath)
      next.activeItemScenePath = copyPath(ownerScenePath)
      next.startPoint = { x: startX, y: startY }
      next.currentPoint = { x: startX, y: startY }
      state = next
    },
    beginMoveEmbed(ownerScenePath, embedInstanceId, startX, startY) {
      const next = emptyState()
      next.mode = 'moveEmbed'
      next.activeItemId = embedInstanceId
      next.activeItemKind = 'embed'
      next.ownerScenePath = copyPath(ownerScenePath)
      next.activeItemScenePath = copyPath(ownerScenePath)
      next.startPoint = { x: startX, y: startY }
      next.currentPoint = { x: startX, y: startY }
      state = next
    },
    beginPinch(centerX, centerY) {
      const next = emptyState()
      next.mode = 'pinch'
      // #818：双指只有一台全局相机，没有 owner 参数，归属恒为根
      next.ownerScenePath = []
      next.startPoint = { x: centerX, y: centerY }
      next.currentPoint = { x: centerX, y: centerY }
      state = next
    },
    endPinch() {
      if (state.mode !== 'pinch') return
      state = emptyState()
    },
    updateCurrent(x, y) { state.currentPoint = { x, y } },
    setTargetItem(itemId) { state.targetItemId = itemId },
    setTargetItemScenePath(scenePath) { state.targetItemScenePath = copyPath(scenePath) },
    reset() { state = emptyState() },
    isIdle() { return state.mode === 'idle' },
    isDraggingNode() { return state.mode === 'moveNode' || state.mode === 'moveEmbed' },
    isConnecting() { return state.mode === 'connect' },
    isPinching() { return state.mode === 'pinch' },
    isPanOwnedByScene(scenePath) {
      if (state.panOwnerScenePath === null) return false
      return sameScenePath(state.panOwnerScenePath, scenePath)
    },
    isConnectOwnedByScene(scenePath) {
      if (state.connectOwnerScenePath === null) return false
      return sameScenePath(state.connectOwnerScenePath, scenePath)
    },
    isActiveItemInScene(scenePath) {
      if (state.activeItemScenePath === null) return false
      return sameScenePath(state.activeItemScenePath, scenePath)
    },
    isOwnedByScene(scenePath) { return sameScenePath(state.ownerScenePath, scenePath) },
    getState() { return JSON.parse(JSON.stringify(state)) }
  }
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
function near(a, b) {
  return Math.abs(a - b) <= 1e-9 * Math.max(1, Math.abs(a), Math.abs(b))
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

console.log('5. Embed 布局：只读 embed.position，平台层定义正圆尺寸')
{
  const embeds = [
    { instanceId: 'emb1', targetStarmapId: 'sm2', label: '支线', position: { x: 640, y: 48 } }
  ]
  const layout = buildEmbedLayoutNodes(embeds)
  assert(layout.length === 1, '嵌入进布局')
  assert(layout[0].nodeId === 'emb1', 'Embed 布局 id 用 instanceId')
  assert(layout[0].x === 640 && layout[0].y === 48, 'Embed 位置来自 embed.position')
  assert(layout[0].width === DEFAULT_EMBED_DIAMETER && layout[0].height === DEFAULT_EMBED_DIAMETER,
    'Embed 宽高都是同一个直径（正圆）')
  assert(layout[0].radius === DEFAULT_EMBED_DIAMETER / 2, 'Embed 圆角 = 直径 / 2')
  assert(layout[0].zIndex === 1, 'Embed 层级在普通节点之上')
}

console.log('5b. Embed 布局没有任何档位入口：几何恒定（#821）')
{
  const embeds = [
    { instanceId: 'emb1', targetStarmapId: 'sm2', label: '支线', position: { x: 640, y: 48 } },
    { instanceId: 'emb2', targetStarmapId: 'sm3', label: '支线二', position: { x: 200, y: 300 } }
  ]
  const layout = buildEmbedLayoutNodes(embeds)
  assert(buildEmbedLayoutNodes.length === 1,
    'buildEmbedLayoutNodes 只接 embeds 一个入参（没有显示边界 / LOD 入口）')
  assert(layout[0].width === DEFAULT_EMBED_DIAMETER && layout[0].radius === DEFAULT_EMBED_DIAMETER / 2,
    '第一颗 Embed 是基准正圆')
  assert(layout[1].width === DEFAULT_EMBED_DIAMETER && layout[1].height === DEFAULT_EMBED_DIAMETER &&
    layout[1].radius === DEFAULT_EMBED_DIAMETER / 2,
    '第二颗 Embed 同样是基准正圆：掉档不改形状')
  assert(layout[0].collapsed === false && layout[1].collapsed === false,
    'collapsed 恒为 false（不存在矩形摘要卡）')
  assert(layout[1].x === 200 && layout[1].y === 300,
    '位置直接就是 authored position，不再做中心锚点派生')
  assert(layout[1].x + layout[1].width / 2 === 200 + DEFAULT_EMBED_DIAMETER / 2,
    '圆心与 authored 圆心重合（派生式退化成恒等）')
}

console.log('5c. 掉档 / 换档不重建任何布局矩形（#821）')
{
  const embed = { instanceId: 'emb1', targetStarmapId: 'sm2', position: { x: 640, y: 48 } }
  const before = buildEmbedLayoutNodes([embed])[0]
  // Deep Zoom 档位变化（interactive → preview → shell）在布局层完全不可见：
  // 布局函数不接收档位，也不该因为档位被重新调用出别的矩形。
  for (const _detail of ['interactive', 'preview', 'shell']) {
    const after = buildEmbedLayoutNodes([embed])[0]
    assert(after.x === before.x && after.y === before.y &&
      after.width === before.width && after.height === before.height &&
      after.radius === before.radius && after.collapsed === before.collapsed,
      `档位 ${_detail} 下 Embed 布局矩形一字不改`)
  }
  // 缩放不碰布局：唯一改变屏幕尺寸的是显示变换（.scale）
  const scaled = DEFAULT_EMBED_DIAMETER * 40
  assert(before.width === DEFAULT_EMBED_DIAMETER && scaled > before.width * 39,
    '放大 40 倍只发生在显示变换上，布局宽度始终是基准直径（不会撑出巨型缓冲）')
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

console.log('7. 缩放：乘法步进 + 数值安全边界，没有"返回父层"分支（#821）')
{
  assert(near(zoomOut(0.5), 0.5 / ZOOM_FACTOR), '0.5 按倍数缩小')
  assert(near(zoomIn(0.5), 0.5 * ZOOM_FACTOR), '0.5 按倍数放大')
  // 相对步进一致：加法步长在两端手感完全不同，这是它被换掉的原因
  assert(near(zoomIn(2) / 2, ZOOM_FACTOR) && near(zoomIn(1) / 1, ZOOM_FACTOR),
    '放大是相对步进：1× 和 2× 下的增幅比例一致（加法步长做不到这点）')
  // 边界是数值安全界，不是产品天花板：3× 这种硬顶不再存在
  assert(zoomIn(3) > 3, '超过旧的 3× 天花板仍可继续放大（#821 核心目标）')
  assert(zoomIn(1e4) > 1e4, '可以放到 10000× 级别，不会被 3 卡住')
  assert(zoomOut(1e4) > 1000 && zoomOut(zoomOut(1e4)) > 600,
    '缩小同样没有地板式硬顶：0.3 那种下限退得回去')
  // 真正的边界只防数值事故
  assert(zoomIn(CAMERA_SCALE_MAX) === CAMERA_SCALE_MAX, '放大夹在数值上界')
  assert(zoomOut(CAMERA_SCALE_MIN) === CAMERA_SCALE_MIN, '缩小夹在数值下界')
  assert(clampCameraScale(NaN) === 1 && clampCameraScale(0) === 1 && clampCameraScale(-2) === 1,
    '退化输入退回 1，不把 NaN / 0 / 负数写进相机')
  // 双指捏合保持连续比例，边界同样交给 clamp
  assert(near(applyPinchScale(2, 1.5), 3), '双指放大按基准比例 × 手势比例')
  assert(near(applyPinchScale(2, 0.25), 0.5), '双指缩小同理')
  assert(applyPinchScale(1, 1e6) === CAMERA_SCALE_MAX, '双指疯捏夹在上界')
  assert(applyPinchScale(1, 1e-6) === CAMERA_SCALE_MIN, '双指疯开夹在下界')
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

console.log('9. hitTestWithScene：普通节点命中 → node/body')
{
  const rects = [
    { nodeId: 'n1', x: 0, y: 0, width: 160, height: 80 },
    { nodeId: 'n2', x: 200, y: 100, width: 160, height: 80 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set()
  const hit = hitTestWithScene(rects, 50, 40, scenePath, embedIds, 1)
  assert(hit !== null, '命中节点返回非 null')
  assert(hit.objectKind === 'node', 'objectKind = node')
  assert(hit.hitRegion === 'body', 'hitRegion = body')
  assert(hit.objectId === 'n1', 'objectId = n1')
  assert(eq(hit.scenePath, scenePath), 'scenePath 正确传递')
}

console.log('10. hitTestWithScene：Embed title 命中 → embedTitle/title')
{
  // Embed 是正圆：外接矩形 (100, 50, 200×200)，圆心 (200, 150)，半径 100
  const rects = [
    { nodeId: 'emb1', x: 100, y: 50, width: 200, height: 200 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set(['emb1'])
  // title 区域：圆内顶部 EMBED_TITLE_HIT_HEIGHT(24) 高度，y ∈ [50, 74]
  const hit = hitTestWithScene(rects, 200, 60, scenePath, embedIds, 1)
  assert(hit !== null, '命中 Embed title 区域返回非 null')
  assert(hit.objectKind === 'embedTitle', 'objectKind = embedTitle')
  assert(hit.hitRegion === 'title', 'hitRegion = title')
  assert(hit.objectId === 'emb1', 'objectId = emb1')
  assert(eq(hit.scenePath, scenePath), 'scenePath 正确传递')
}

console.log('11. hitTestWithScene：Embed 圆环 border 命中 → embedBorder/border')
{
  const rects = [
    { nodeId: 'emb1', x: 100, y: 50, width: 200, height: 200 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set(['emb1'])
  // 圆环：到圆心距离 >= 半径 - EMBED_BORDER_HIT_WIDTH = 88
  // 正左：圆心正左 100 → 距离 100（圆周）
  const hitLeft = hitTestWithScene(rects, 100, 150, scenePath, embedIds, 1)
  assert(hitLeft !== null && hitLeft.objectKind === 'embedBorder' && hitLeft.hitRegion === 'border',
    '圆周正左点 → embedBorder/border')
  // 正下：圆心正下 100 → 距离 100
  const hitBottom = hitTestWithScene(rects, 200, 250, scenePath, embedIds, 1)
  assert(hitBottom !== null && hitBottom.objectKind === 'embedBorder' && hitBottom.hitRegion === 'border',
    '圆周正下点 → embedBorder/border')
  // 斜向圆环：圆心 + (70, 70) → 距离 ≈ 98.99，落在 [88, 100] 圆环内
  const hitDiagonal = hitTestWithScene(rects, 270, 220, scenePath, embedIds, 1)
  assert(hitDiagonal !== null && hitDiagonal.objectKind === 'embedBorder' && hitDiagonal.hitRegion === 'border',
    '斜向圆周点 → embedBorder/border（不是矩形边）')
  // 交互热区 12vp：距离 89 的点（正好在内切圆环内侧一点）仍算边框，
  // 说明热区比视觉描边（1/3vp）宽得多，手指点得到
  const hitInsideRing = hitTestWithScene(rects, 200, 239, scenePath, embedIds, 1)
  assert(hitInsideRing !== null && hitInsideRing.objectKind === 'embedBorder',
    '距离 89（圆环内侧）→ 仍是 embedBorder，交互热区 12vp 宽于视觉描边')
  // 圆环内切边界再往里一点：距离 87 已经是子图内部
  const hitInnerEdge = hitTestWithScene(rects, 200, 237, scenePath, embedIds, 1)
  assert(hitInnerEdge !== null && hitInnerEdge.objectKind === 'embedInnerContent',
    '距离 87（越过圆环内切）→ embedInnerContent')
}

console.log('12. hitTestWithScene：圆外方形角不再命中 Embed')
{
  const rects = [
    { nodeId: 'emb1', x: 100, y: 50, width: 200, height: 200 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set(['emb1'])
  // 外接矩形的左上角 (100, 50) 离圆心 √(100²+100²) > 100，不在圆内
  assert(hitTestWithScene(rects, 100, 50, scenePath, embedIds, 1) === null,
    '正方形角（矩形内、圆外）不命中 Embed')
  // 外接矩形右下角同理
  assert(hitTestWithScene(rects, 300, 250, scenePath, embedIds, 1) === null,
    '右下角（矩形内、圆外）不命中 Embed')
  // 矩形外侧更远处也不命中
  assert(hitTestWithScene(rects, 320, 150, scenePath, embedIds, 1) === null,
    '矩形右侧外部不命中 Embed')
}

console.log('13. hitTestWithScene：Embed innerContent 命中 → embedInnerContent/innerContent')
{
  const rects = [
    { nodeId: 'emb1', x: 100, y: 50, width: 200, height: 200 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set(['emb1'])
  // 圆心附近：y > 74（title 之下）且到圆心距离 < 88（不在圆环上）
  const hit = hitTestWithScene(rects, 200, 150, scenePath, embedIds, 1)
  assert(hit !== null, '命中 Embed innerContent 返回非 null')
  assert(hit.objectKind === 'embedInnerContent', 'objectKind = embedInnerContent')
  assert(hit.hitRegion === 'innerContent', 'hitRegion = innerContent')
  assert(hit.objectId === 'emb1', 'objectId = emb1')
}

console.log('14. hitTestWithScene：未命中 → null')
{
  const rects = [
    { nodeId: 'n1', x: 0, y: 0, width: 160, height: 80 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set()
  const hit = hitTestWithScene(rects, 500, 500, scenePath, embedIds, 1)
  assert(hit === null, '点击在所有 rect 之外 → null')
}

console.log('15. hitTestWithScene：后绘制（数组末尾）的 rect 优先命中')
{
  const rects = [
    { nodeId: 'n1', x: 0, y: 0, width: 200, height: 200 },
    { nodeId: 'n2', x: 0, y: 0, width: 200, height: 200 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set()
  const hit = hitTestWithScene(rects, 100, 100, scenePath, embedIds, 1)
  assert(hit.objectId === 'n2', '后绘制的 n2 优先命中（zIndex 更高）')
}

console.log('16. hitTestWithScene：上层 Embed 圆外时穿透到下层节点')
{
  // emb1 圆形覆盖了 n1 的一部分；emb1 圆外的点应命中下层 n1
  const rects = [
    { nodeId: 'n1', x: 0, y: 0, width: 300, height: 300 },
    { nodeId: 'emb1', x: 100, y: 100, width: 200, height: 200 }
  ]
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const embedIds = new Set(['emb1'])
  // (200, 200) 在 emb1 圆内 → 命中 emb1
  assert(hitTestWithScene(rects, 200, 200, scenePath, embedIds, 1).objectId === 'emb1',
    '圆内点命中上层 Embed')
  // (10, 10) 在 emb1 圆外但仍在 n1 矩形内 → 命中 n1
  const hit = hitTestWithScene(rects, 10, 10, scenePath, embedIds, 1)
  assert(hit !== null && hit.objectId === 'n1',
    'Embed 圆外不再吞掉事件，穿透命中下层节点 n1')
}

console.log('17. 边端点：Embed 端点落在圆周上，不是矩形边')
{
  // 左侧普通节点 (0, 0, 160×80)，中心 (80, 40)
  // 右侧圆形 Embed (400, 0, 200×200)，中心 (500, 100)，半径 100
  const nodeRect = { nodeId: 'n1', x: 0, y: 0, width: 160, height: 80 }
  const embedRect = { nodeId: 'emb1', x: 400, y: 0, width: 200, height: 200 }
  const pts = computeEdgeEndpoints(nodeRect, embedRect, false, true)
  // 终点必须在圆周上：到圆心距离 == 半径
  const dEnd = Math.hypot(pts.end.x - 500, pts.end.y - 100)
  assert(Math.abs(dEnd - 100) < 1e-6, `Embed 端点在圆周上（距离 ${dEnd.toFixed(3)} ≈ 100）`)
  // 圆周边的斜向点：y 必然大于矩形上边 y=100 之下？不，矩形上边是 y=0，
  // 关键断言：终点的 y 落在圆周上，而矩形边界交点会落在 y=0 或 x=400 上
  assert(pts.end.y > 0, `Embed 端点不在矩形上边 y=0（y=${pts.end.y.toFixed(2)}）`)
  // 起点仍是普通节点的矩形边界：到节点中心的连线与矩形相交
  assert(pts.start.x === 160, `普通节点端点仍在矩形右边 x=160（x=${pts.start.x}）`)
}

console.log('18. 边端点：Embed → Embed 两侧都在圆周上')
{
  const embedA = { nodeId: 'embA', x: 0, y: 0, width: 200, height: 200 }
  const embedB = { nodeId: 'embB', x: 600, y: 400, width: 200, height: 200 }
  const pts = computeEdgeEndpoints(embedA, embedB, true, true)
  const dStart = Math.hypot(pts.start.x - 100, pts.start.y - 100)
  const dEnd = Math.hypot(pts.end.x - 700, pts.end.y - 500)
  assert(Math.abs(dStart - 100) < 1e-6, `起点在 embA 圆周上（距离 ${dStart.toFixed(3)} ≈ 100）`)
  assert(Math.abs(dEnd - 100) < 1e-6, `终点在 embB 圆周上（距离 ${dEnd.toFixed(3)} ≈ 100）`)
}

console.log('19. lineCircleIntersection：目标在圆内也拉到圆周，方向不变')
{
  // 目标点比半径近：应沿同方向推到圆周
  const near = lineCircleIntersection(100, 100, 100, 120, 100)
  assert(near.x === 200 && near.y === 100, '圆内目标拉到圆周 (200, 100)')
  // 目标点与圆心重合：没有方向，返回圆心
  const same = lineCircleIntersection(100, 100, 100, 100, 100)
  assert(same.x === 100 && same.y === 100, '目标与圆心重合时返回圆心')
}

console.log('20. 拉线预览起点：与正式边共用 edgeEndpointBoundaryPoint，不从中心出发')
{
  // 圆形 Embed (400, 0, 200×200)，中心 (500, 100)，半径 100
  const embedRect = { nodeId: 'emb1', x: 400, y: 0, width: 200, height: 200 }
  // 手指画布坐标在圆的右侧 (800, 100)：预览起点必须是圆周右端 (600, 100)
  const previewStart = edgeEndpointBoundaryPoint(embedRect, true, 800, 100)
  assert(Math.abs(previewStart.x - 600) < 1e-6 && Math.abs(previewStart.y - 100) < 1e-6,
    `Embed 预览起点在圆周上（${previewStart.x}, ${previewStart.y}）`)
  assert(!(previewStart.x === 500 && previewStart.y === 100),
    'Embed 预览起点不是圆心')
  // 普通节点同样走这个入口，起点落在矩形边界而不是中心
  const nodeRect = { nodeId: 'n1', x: 0, y: 0, width: 160, height: 80 }
  const nodePreviewStart = edgeEndpointBoundaryPoint(nodeRect, false, 800, 40)
  assert(nodePreviewStart.x === 160,
    `普通节点预览起点在矩形边界 x=160（x=${nodePreviewStart.x}）`)
}

console.log('21. buildCircleResponseBands：热区分带覆盖整个圆且不越出圆外')
{
  const bands = buildCircleResponseBands(DEFAULT_EMBED_DIAMETER)
  const expectedBands = Math.ceil(DEFAULT_EMBED_DIAMETER / MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP)
  // 内切方案会丢掉极点处宽度为 0 的两条带
  assert(bands.length === expectedBands - 2,
    `按最大带高 ${MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP}vp 分 ${expectedBands} 带，丢弃 2 条极点带（实际 ${bands.length}）`)
  // 每条带高都不超过最大带高
  assert(bands.every(b => b.height <= MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP + 1e-9),
    '每条带高不超过 MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP')
  // 所有带必须落在 [0, 直径] 内，且水平居中于圆心 x=100
  let inside = true
  let centered = true
  for (const b of bands) {
    if (b.x < 0 || b.y < 0 || b.x + b.width > DEFAULT_EMBED_DIAMETER + 1e-6 ||
        b.y + b.height > DEFAULT_EMBED_DIAMETER + 1e-6) inside = false
    if (Math.abs((b.x + b.width / 2) - DEFAULT_EMBED_DIAMETER / 2) > 1e-6) centered = false
  }
  assert(inside, '所有热区带都在直径范围内')
  assert(centered, '所有热区带水平居中于圆心')
  // 内切不变式：任意带的任意位置都不得越出圆外
  let maxOutside = -Infinity
  for (const b of bands) {
    for (const edgeY of [b.y, b.y + b.height]) {
      const exact = circleHalfWidthAtY(DEFAULT_EMBED_DIAMETER / 2, edgeY)
      maxOutside = Math.max(maxOutside, b.width / 2 - exact)
    }
  }
  assert(maxOutside <= 1e-9, `内切方案下热区不越出圆（最大外凸 ${maxOutside.toExponential(2)}）`)
}

console.log('22. buildCircleResponseBands：圆外方角不再属于热区')
{
  const bands = buildCircleResponseBands(DEFAULT_EMBED_DIAMETER)
  const r = DEFAULT_EMBED_DIAMETER / 2
  // 命中判定：点落在任意一条带内即认为属于热区
  const inResponseRegion = (px, py) => bands.some(
    b => px >= b.x && px <= b.x + b.width && py >= b.y && py <= b.y + b.height
  )
  // 圆心的四个方角 (0,0) (200,0) (0,200) (200,200)：视觉上是圆外，必须不可点
  assert(!inResponseRegion(0, 0) && !inResponseRegion(200, 0) &&
         !inResponseRegion(0, 200) && !inResponseRegion(200, 200),
    '四个方角都不在圆形热区内')
  // 圆心和左右两个边界中点：必须在热区内
  assert(inResponseRegion(100, 100), '圆心在热区内')
  assert(inResponseRegion(1, 100) && inResponseRegion(199, 100),
    '左右边界中点都在热区内')
  // 圆外远处明确不可点
  assert(!inResponseRegion(0, 0) && !inResponseRegion(-1, 100),
    '圆外点不在热区内')
  // 内切方案下极点处宽度为 0，最上和最下各有一条带被丢弃（各一条带高）。
  // 刻意的取舍：宁可漏掉极点一条窄带，也不能把圆外的点算成圆内。
  const bandHeight = DEFAULT_EMBED_DIAMETER /
    Math.ceil(DEFAULT_EMBED_DIAMETER / MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP)
  const polarInset = 2 * bandHeight
  assert(inResponseRegion(100, polarInset) && inResponseRegion(100, DEFAULT_EMBED_DIAMETER - polarInset),
    `圆顶/圆底内缩 ${polarInset.toFixed(2)}vp 后可点`)
  const all = buildCircleResponseBands(DEFAULT_EMBED_DIAMETER)
  assert(all[0].y === bandHeight, `最顶一条带从第 2 带开始（y=${all[0].y.toFixed(2)}）`)
  assert(all[0].width / 2 < DEFAULT_EMBED_DIAMETER / 2 - 1,
    `最顶一条带的半宽 ${(all[0].width / 2).toFixed(2)}vp 仍明显小于半径`)
  assert(all[all.length - 1].width / 2 < DEFAULT_EMBED_DIAMETER / 2 - 1,
    `最底一条带的半宽 ${(all[all.length - 1].width / 2).toFixed(2)}vp 仍明显小于半径`)
}

console.log('23. buildCircleResponseBands：丢弃的圆帽高度不随缩放放大')
{
  // 写死带数时圆帽高度 = 直径/带数，3x 时会涨到 18.75vp（看得见点不到）。
  // 按最大带高分带后，无论缩放多少，圆帽都只有一个固定上限。
  for (const zoom of [0.3, 1, 2, 3]) {
    const diameter = DEFAULT_EMBED_DIAMETER * zoom
    const bands = buildCircleResponseBands(diameter)
    // 圆帽高度 = 第一条带的 y（极点那条被丢弃）
    const capHeight = bands[0].y
    assert(capHeight <= MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP + 1e-9,
      `zoom=${zoom}（直径 ${diameter}）圆帽高度 ${capHeight.toFixed(3)}vp <= 上限 ${MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP}vp`)
    // 对比：旧算法（固定 32 带）在 3x 时圆帽高达 18.75vp
    if (zoom === 3) {
      const legacyCap = diameter / 32
      assert(legacyCap > MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP * 4,
        `对照：固定 32 带在 3x 时圆帽 ${legacyCap.toFixed(2)}vp，远超上限`)
    }
  }
  // 带数随直径线性增长是已知代价，明确记录一下量级
  const at3x = buildCircleResponseBands(DEFAULT_EMBED_DIAMETER * 3).length
  assert(at3x === Math.ceil(DEFAULT_EMBED_DIAMETER * 3 / MAX_CIRCLE_RESPONSE_BAND_HEIGHT_VP) - 2,
    `3x 下带数 ${at3x} 条（直径/最大带高 − 2 条极点带）`)
}

console.log('24. buildCircleResponseBands：退化输入不产生热区')
{
  assert(buildCircleResponseBands(0).length === 0, '直径 0 不产生热区')
  assert(buildCircleResponseBands(-10).length === 0, '负直径不产生热区')
  assert(buildCircleResponseBands(200, 0).length === 0, '最大带高 0 不产生热区')
  assert(buildCircleResponseBands(200, -3).length === 0, '负最大带高不产生热区')
}

console.log('25. 预览端点与正式边端点在同一形状上一致')
{
  // 同一个 Embed、同一个目标：预览用的 edgeEndpointBoundaryPoint
  // 与正式边 computeEdgeRender 里的端点必须完全相同，不存在两套算法
  const embedRect = { nodeId: 'emb1', x: 0, y: 0, width: 200, height: 200 }
  const nodeRect = { nodeId: 'n1', x: 600, y: 0, width: 160, height: 80 }
  const formal = computeEdgeEndpoints(nodeRect, embedRect, false, true)
  const preview = edgeEndpointBoundaryPoint(embedRect, true, 680, 40)
  assert(eq(preview, formal.end),
    `预览起点与正式边终点同源（预览 ${JSON.stringify(preview)}，正式 ${JSON.stringify(formal.end)}）`)
}

console.log('26. GestureState：beginPanCanvas 传入 ownerScenePath')
{
  const tracker = createGestureStateTracker()
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  tracker.beginPanCanvas(scenePath, 10, 20)
  const s = tracker.getState()
  assert(s.mode === 'panCanvas', 'mode = panCanvas')
  assert(eq(s.ownerScenePath, scenePath), 'ownerScenePath 正确')
  assert(s.activeItemId === '', 'panCanvas 无 activeItemId')
  assert(s.activeItemKind === 'node', 'panCanvas activeItemKind = node')
  assert(eq(s.startPoint, { x: 10, y: 20 }), 'startPoint 正确')
}

console.log('27. GestureState：beginNodeMenu 传入 ownerScenePath')
{
  const tracker = createGestureStateTracker()
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  tracker.beginNodeMenu(scenePath, 'n1', 30, 40)
  const s = tracker.getState()
  assert(s.mode === 'nodeMenu', 'mode = nodeMenu')
  assert(eq(s.ownerScenePath, scenePath), 'ownerScenePath 正确')
  assert(s.activeItemId === 'n1', 'activeItemId = n1')
  assert(s.activeItemKind === 'node', 'activeItemKind = node')
}

console.log('28. GestureState：beginConnect 传入 ownerScenePath')
{
  const tracker = createGestureStateTracker()
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  tracker.beginConnect(scenePath, 'n2', 50, 60)
  const s = tracker.getState()
  assert(s.mode === 'connect', 'mode = connect')
  assert(eq(s.ownerScenePath, scenePath), 'ownerScenePath 正确')
  assert(s.activeItemId === 'n2', 'activeItemId = n2')
  assert(s.activeItemKind === 'node', 'activeItemKind = node')
}

console.log('29. GestureState：beginMoveNode 传入 ownerScenePath')
{
  const tracker = createGestureStateTracker()
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  tracker.beginMoveNode(scenePath, 'n3', 70, 80)
  const s = tracker.getState()
  assert(s.mode === 'moveNode', 'mode = moveNode')
  assert(eq(s.ownerScenePath, scenePath), 'ownerScenePath 正确')
  assert(s.activeItemId === 'n3', 'activeItemId = n3')
  assert(s.activeItemKind === 'node', 'activeItemKind = node')
}

console.log('30. GestureState：beginMoveEmbed 传入 ownerScenePath')
{
  const tracker = createGestureStateTracker()
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  tracker.beginMoveEmbed(scenePath, 'emb1', 90, 100)
  const s = tracker.getState()
  assert(s.mode === 'moveEmbed', 'mode = moveEmbed')
  assert(eq(s.ownerScenePath, scenePath), 'ownerScenePath 正确')
  assert(s.activeItemId === 'emb1', 'activeItemId = emb1')
  assert(s.activeItemKind === 'embed', 'activeItemKind = embed')
}

console.log('31. GestureState：activeItemKind 区分 node vs embed')
{
  const tracker = createGestureStateTracker()
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  tracker.beginMoveNode(scenePath, 'n1', 0, 0)
  assert(tracker.getState().activeItemKind === 'node', 'beginMoveNode → activeItemKind = node')
  tracker.beginMoveEmbed(scenePath, 'emb1', 0, 0)
  assert(tracker.getState().activeItemKind === 'embed', 'beginMoveEmbed → activeItemKind = embed')
  tracker.beginNodeMenu(scenePath, 'n2', 0, 0)
  assert(tracker.getState().activeItemKind === 'node', 'beginNodeMenu → activeItemKind = node')
  tracker.beginConnect(scenePath, 'n3', 0, 0)
  assert(tracker.getState().activeItemKind === 'node', 'beginConnect → activeItemKind = node')
  tracker.beginPanCanvas(scenePath, 0, 0)
  assert(tracker.getState().activeItemKind === 'node', 'beginPanCanvas → activeItemKind = node')
}

console.log('32. GestureState：reset() 清空 ownerScenePath 和 activeItemKind')
{
  const tracker = createGestureStateTracker()
  const scenePath = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  tracker.beginMoveEmbed(scenePath, 'emb1', 10, 20)
  assert(tracker.getState().ownerScenePath.length === 1, 'reset 前有 ownerScenePath')
  assert(tracker.getState().activeItemKind === 'embed', 'reset 前 activeItemKind = embed')
  tracker.reset()
  const s = tracker.getState()
  assert(s.mode === 'idle', 'reset 后 mode = idle')
  assert(eq(s.ownerScenePath, []), 'reset 后 ownerScenePath 为空数组')
  assert(s.activeItemKind === 'node', 'reset 后 activeItemKind 回到默认 node')
  assert(s.activeItemId === '', 'reset 后 activeItemId 为空')
}

console.log('33. GestureState：isOwnedByScene 正确比较路径')
{
  const tracker = createGestureStateTracker()
  const scenePath1 = [{ type: 'starmap', instanceId: 'sm1', nodeId: '' }]
  const scenePath2 = [{ type: 'starmap', instanceId: 'sm2', nodeId: '' }]
  const scenePath3 = [
    { type: 'starmap', instanceId: 'sm1', nodeId: '' },
    { type: 'embed', instanceId: 'emb1', nodeId: 'n1' }
  ]
  tracker.beginMoveNode(scenePath1, 'n1', 0, 0)
  assert(tracker.isOwnedByScene(scenePath1) === true, '相同路径 → true')
  assert(tracker.isOwnedByScene(scenePath2) === false, '不同 instanceId → false')
  assert(tracker.isOwnedByScene(scenePath3) === false, '不同长度路径 → false')
  tracker.reset()
  assert(tracker.isOwnedByScene(scenePath1) === false, 'reset 后空路径不匹配任何路径')
}

console.log('34. GestureState：归属按手势类型分开记（#818 pinch 无归属）')
{
  const tracker = createGestureStateTracker()
  const root = []
  const child = [{ type: 'enterEmbed', instanceId: 'emb1', nodeId: null }]

  // #818：双指只有一台全局相机，没有 owner 参数，缩放目标不随手指落点变化。
  tracker.beginPinch(100, 100)
  assert(tracker.isPinching() === true, 'beginPinch → isPinching')
  assert(eq(tracker.getState().ownerScenePath, root), 'pinch 归属恒为根（统一入口在根 Stack）')
  assert(tracker.isPanOwnedByScene(child) === false, 'pinch 不串到 pan 归属')
  assert(tracker.isConnectOwnedByScene(child) === false, 'pinch 不串到 connect 归属')
  assert(tracker.isActiveItemInScene(child) === false, 'pinch 没有操作对象')
  assert(tracker.getState().pinchOwnerScenePath === undefined, '状态里不再有 pinchOwnerScenePath')

  // 双指进行中：任何一层都不该顺手改视口（相机唯一，改了就是和 pinch 打架）
  tracker.endPinch()
  assert(tracker.isPinching() === false, '双指抬起后结束')

  tracker.beginConnect(child, 'n1', 0, 0)
  assert(tracker.isConnectOwnedByScene(child) === true, 'connect 归属发起层')
  assert(eq(tracker.getState().connectSourceScenePath, child), 'connect 起点 Scene 单独记一份')
  tracker.setTargetItemScenePath(root)
  assert(eq(tracker.getState().targetItemScenePath, root), '终点 Scene 单独记一份（终点可能在父层）')

  tracker.beginMoveEmbed(child, 'emb1', 5, 5)
  assert(tracker.isActiveItemInScene(child) === true, '操作对象归属所在层')
  assert(tracker.getState().activeItemKind === 'embed', '操作对象类型是 embed')
  assert(tracker.isDraggingNode() === true, 'moveEmbed 属于拖拽节点')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
