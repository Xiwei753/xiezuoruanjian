import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const __testDir = dirname(fileURLToPath(import.meta.url))
const STARMAP_DIR = join(__testDir, '..')
/** 读真实 .ets 源文件，做结构守卫（避免属性装饰器被悄悄改回普通字段） */
function readStarmapSource(relativePath) {
  return readFileSync(join(STARMAP_DIR, relativePath), 'utf8')
}

// starmap_recursive_scene.test.mjs — 星图递归 Scene 层（Issue #816）纯逻辑测试。
//
// 纯 JS（.mjs），不依赖 ArkUI / Core bridge，Node 直接运行：
//   node apps/harmony/entry/src/main/ets/feature/starmap/__tests__/starmap_recursive_scene.test.mjs
//
// 本评论要求的行为契约（对应实现文件）：
//   1. 子图初始比例来自"内容包围盒 × 父 Embed 圆形可用区"，这是**布局适配**（局部 fit），
//      不是用户缩放；不允许出现"每深一层乘 0.6"这种写死系数。
//      局部 fit 存的是**局部数据**：算之前先把 sceneWidth / sceneHeight 除掉祖先累计比例，
//      所以同一张子图在相机 1 和相机 2 下打开算出的 fit 完全一样
//      —— computeContentBounds / computeFitScale / computeFittedViewport (StarMapViewport.ets)
//   2. 缩放只有一台相机：双指捏合与工具栏 +/− 都只改**全局视角相机**，整棵递归星图一起缩放；
//      锚点按 #818 的公式落在根画布的世界坐标里，任何一层都不允许单独被缩放
//      —— clampCameraScale / computeCameraPinch / computeCameraZoomAround (StarMapViewport.ets)
//   2b. 相机要真的作用到整棵树：子 Scene 的有效比例 = `fitScale × inheritedScale`
//      （inheritedScale = 父 Scene 的有效比例），偏移同理。
//      否则相机放大时只有 Embed 圆壳变大，圆里的节点不跟着放大 —— 只是把容器放大，不是缩放视角
//      —— viewportScaleValue / viewportOffsetX / viewportOffsetY / localSceneSize
//         (ui/StarMapScene.ets)
//
//   3. 递归 Scene 之间的坐标换算必须扣掉 Embed 矩形自己的原点。
//      子 Scene 的画布坐标是它自己那张图的 node.position，和父画布没有共同数值范围——
//      直接把父画布坐标塞进子视口换算，缩放中心会跑到子图外面去（#816 真机症状的根因）
//      —— embedParentCanvasToChildLocal / childLocalToParentCanvas / resolveSceneLocalPoint
//         / convertPointToRoot (StarMapGeometry.ets)
//
//   4. 递归命中：从根一路解析到最深的真实目标。命中普通节点停在本层；命中 Embed 标题/边框停在本层
//      且目标是 Embed；命中 Embed 内部且子图已加载就换算到子 Scene 继续下探；
//      子图未加载停在本层（目标仍是 Embed）；空白则 target 为 null
//      —— resolveRecursiveHit / buildRecursiveSceneContext (StarMapGeometry.ets)
//
//   5. 双指不再需要归属：捏合状态只有一个"进行中"标志，相机由 StarMapScreen 独占。
//      递归命中只服务选中 / 拖拽 / 连线 / 长按菜单，绝不参与决定缩放目标（#818）
//      —— StarMapGestureStateTracker.beginPinch (StarMapGestureState.ets)
//
//   5b. 双指 > 所有单指（#818 复审）：真机上第一根手指落下时单指识别器已经先在竞争，
//      第二根手指才让高优先级 Pinch 认领，所以晚到的单指回调不能再改写 mode。
//      状态机里每个单指 begin* 都要拒绝在 pinch 期间改状态（isPinchingState 守卫）
//      —— beginPanCanvas / beginNodeMenu / beginConnect / beginMoveNode / beginMoveEmbed
//         (StarMapGestureState.ets)
//
//   5c. 子星图不是独立窗口，没有自己的平移相机（#818 复审）：任意一层空白处的普通
//      单指拖动都只改根全局相机，子 Scene 把目标 offset 通过 onCameraPanTo 回传给根，
//      自己的 fitOffsetX/Y 一个像素都不动（那只由 fitView 写）。
//      传的是目标值不是增量：起手记 panBaseCameraOffsetX/Y，更新时算 基准 + event.offset，
//      根整份覆盖；写成 camera += event.offset 会把同一手势的每一帧叠加一遍。
//      —— applyCameraPanTo (ui/StarMapScene.ets) / panCameraTo (ui/StarMapScreen.ets)
//
//   5d. 子星图里的对象不能被拖到圆外（#818 复审）：写入前夹进圆的内接正方形安全区，
//      夹完的坐标既进布局也存进 Core。复用 fitView 的可用区口径，不另造安全区常量
//      —— clampItemToEmbedSafeArea / clampItemToLocalSafeArea
//         (StarMapViewport.ets / ui/StarMapScene.ets)
//
//   5e. 相机状态链上不能有断点（#818 复审）：ArkUI V1 只有 @Prop 是父 → 子单向同步，
//      普通字段只是拿父值做一次初始化。StarMapEmbedScene 正处在
//      Screen @State → 根 Scene @Prop → EmbedScene → child Scene @Prop 这条链的中间，
//      那一层一旦写成普通字段，子 Scene 读到的 cameraOffset 就停在初始化时的旧值，
//      而拖画布正是拿它当起手基准 → 全局相机从 100 跳回 5。
//      行为语义：根层拖到 100，再从任意深度子星图起手 Pan，基准必须是 100，
//      第一帧 +5 之后全局必须是 105，绝不能跳回 5
//      —— @Prop cameraScale / cameraOffsetX / cameraOffsetY (ui/StarMapEmbedScene.ets)
//
//   6. 全树唯一选中态：选中身份 = scenePath + kind + itemId。
//      选中子节点时父 Embed 立刻失选；同名 item 在不同层不互相命中
//      —— StarMapSelectionState (StarMapSelectionState.ets)
//
//   7. 跨层连线：宿主 = 两个端点所在 Scene 的最近公共祖先，两端都换算成相对宿主的路径。
//      必须覆盖：父 Node → 子 Node、子 Node → 父 Node、同一 Embed 里两个子 Node、
//      两个不同 Embed 里的 Node、子 Node → 更深一层的子 Embed/Node
//      —— commonScenePathPrefix / buildTargetPathForHost / planCrossLayerEdge
//         (StarMapGeometry.ets)
//
// 被测规格与对应 .ets 内联实现严格一致。

// ══════════════════════════════════════════════════════════════
// 1. 视口数学（StarMapViewport.ets）
// ══════════════════════════════════════════════════════════════

const CIRCLE_INNER_SAFE_RATIO = 1 / Math.SQRT2
const MIN_FIT_SCALE = 0.02
const MAX_FIT_SCALE = 4
const CAMERA_SCALE_MIN = 0.3
const CAMERA_SCALE_MAX = 3
const EMBED_FIT_PADDING_VP = 8
const EMBED_TITLE_HIT_HEIGHT = 24
const EMBED_BORDER_HIT_WIDTH = 12
const DEFAULT_EMBED_DIAMETER = 200
// 与 platform/StarMapLayout.ets 导出的常量保持一致（UI 侧不手写尺寸）
const DEFAULT_NODE_WIDTH = 160
const DEFAULT_NODE_HEIGHT = 80

function computeContentBounds(rects) {
  if (rects.length === 0) { return null }
  let minX = rects[0].x
  let minY = rects[0].y
  let maxX = rects[0].x + rects[0].width
  let maxY = rects[0].y + rects[0].height
  for (let i = 1; i < rects.length; i++) {
    const r = rects[i]
    if (r.x < minX) { minX = r.x }
    if (r.y < minY) { minY = r.y }
    if (r.x + r.width > maxX) { maxX = r.x + r.width }
    if (r.y + r.height > maxY) { maxY = r.y + r.height }
  }
  return { minX, minY, maxX, maxY, width: maxX - minX, height: maxY - minY }
}

function computeFitScale(bounds, availableWidth, availableHeight, paddingVp) {
  const usableWidth = Math.max(1, availableWidth - paddingVp * 2)
  const usableHeight = Math.max(1, availableHeight - paddingVp * 2)
  const raw = Math.min(usableWidth / bounds.width, usableHeight / bounds.height)
  if (!isFinite(raw) || raw <= 0) { return MIN_FIT_SCALE }
  return Math.max(MIN_FIT_SCALE, Math.min(MAX_FIT_SCALE, raw))
}

// 缩放比例按可用区（子星图里是圆的内接正方形）算；
// 偏移必须按 Scene 自己的尺寸算——子 Scene 组件铺满整个 Embed 圆，
// 中心是 sceneWidth/2、sceneHeight/2，不是内接正方形的中心（第 4 条复审）。
function computeCenteredOffset(bounds, fitScale, sceneWidth, sceneHeight) {
  const centerX = (bounds.minX + bounds.maxX) / 2
  const centerY = (bounds.minY + bounds.maxY) / 2
  return { x: sceneWidth / 2 - centerX * fitScale, y: sceneHeight / 2 - centerY * fitScale }
}

function computeFittedViewport(bounds, availableWidth, availableHeight, paddingVp, sceneWidth, sceneHeight) {
  const fitScale = computeFitScale(bounds, availableWidth, availableHeight, paddingVp)
  const offset = computeCenteredOffset(bounds, fitScale, sceneWidth, sceneHeight)
  return { zoomScale: fitScale, offsetX: offset.x, offsetY: offset.y }
}

// ─── 全局视角相机（#818）───

function clampCameraScale(scale) {
  if (!isFinite(scale) || scale <= 0) { return 1 }
  return Math.max(CAMERA_SCALE_MIN, Math.min(CAMERA_SCALE_MAX, scale))
}

function computeCameraPinch(camera, baseScale, baseDistance, currentDistance, centerX, centerY) {
  const ratio = baseDistance > 0 ? currentDistance / baseDistance : 1
  const nextScale = clampCameraScale(baseScale * ratio)
  const anchorWorldX = (centerX - camera.offsetX) / camera.scale
  const anchorWorldY = (centerY - camera.offsetY) / camera.scale
  return {
    scale: nextScale,
    offsetX: centerX - anchorWorldX * nextScale,
    offsetY: centerY - anchorWorldY * nextScale
  }
}

function computeCameraZoomAround(camera, centerX, centerY, nextScale) {
  const clamped = clampCameraScale(nextScale)
  const anchorWorldX = (centerX - camera.offsetX) / camera.scale
  const anchorWorldY = (centerY - camera.offsetY) / camera.scale
  return {
    scale: clamped,
    offsetX: centerX - anchorWorldX * clamped,
    offsetY: centerY - anchorWorldY * clamped
  }
}

// ─── 子星图内容安全区（#818 复审）───
// 父 Embed 必须留出"标题带 + 圆环"这层交互壳，否则内容一 fit 就贴满内接正方形，
// 父圆自己的标题和边框既露不出来也点不到。扣掉这层壳之后 fit / clamp / 新建落点
// 共用同一份可用区，孙层自然小于子层，不靠固定 depth 系数。
const EMBED_INTERACTION_SHELL_VP = EMBED_TITLE_HIT_HEIGHT + EMBED_BORDER_HIT_WIDTH

function computeEmbedInnerContentSafeSide(localSceneSize, paddingVp) {
  if (!isFinite(localSceneSize) || localSceneSize <= 0) { return 0 }
  const padding = paddingVp > 0 ? paddingVp : 0
  return Math.max(0, localSceneSize * CIRCLE_INNER_SAFE_RATIO - EMBED_INTERACTION_SHELL_VP - padding * 2)
}

function clampItemToEmbedSafeArea(x, y, width, height, scale, offsetX, offsetY, localSceneSize, paddingVp) {
  const fitScale = scale > 0 ? scale : 1
  if (!isFinite(x) || !isFinite(y) || localSceneSize <= 0) { return { x, y } }
  const itemWidth = width * fitScale
  const itemHeight = height * fitScale
  if (!isFinite(itemWidth) || !isFinite(itemHeight)) { return { x, y } }
  const safeSize = computeEmbedInnerContentSafeSide(localSceneSize, paddingVp)
  const center = localSceneSize / 2
  const halfSafe = safeSize / 2
  const minLeft = center - halfSafe
  const maxLeft = center + halfSafe - itemWidth
  const minTop = center - halfSafe
  const maxTop = center + halfSafe - itemHeight
  const clampedLeft = maxLeft < minLeft
    ? center - itemWidth / 2
    : Math.max(minLeft, Math.min(maxLeft, x * fitScale + offsetX))
  const clampedTop = maxTop < minTop
    ? center - itemHeight / 2
    : Math.max(minTop, Math.min(maxTop, y * fitScale + offsetY))
  return { x: (clampedLeft - offsetX) / fitScale, y: (clampedTop - offsetY) / fitScale }
}

function sceneLocalToCanvas(x, y, scale, offsetX, offsetY) {
  if (scale === 0) { return { x: 0, y: 0 } }
  return { x: (x - offsetX) / scale, y: (y - offsetY) / scale }
}

function canvasToSceneLocal(x, y, scale, offsetX, offsetY) {
  return { x: x * scale + offsetX, y: y * scale + offsetY }
}

function canvasToScreen(canvasX, canvasY, zoomScale, offsetX, offsetY) {
  return { x: canvasX * zoomScale + offsetX, y: canvasY * zoomScale + offsetY }
}

function screenToCanvas(screenX, screenY, zoomScale, offsetX, offsetY) {
  if (zoomScale === 0) { return { x: 0, y: 0 } }
  return { x: (screenX - offsetX) / zoomScale, y: (screenY - offsetY) / zoomScale }
}

// ══════════════════════════════════════════════════════════════
// 2. Scene 路径与端点路径（StarMapGeometry.ets）
// ══════════════════════════════════════════════════════════════

function cloneScenePath(path) {
  return path.map(seg => ({ type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId }))
}

function isSameScenePath(a, b) {
  if (a.length !== b.length) { return false }
  for (let i = 0; i < a.length; i++) {
    if (a[i].type !== b[i].type || a[i].instanceId !== b[i].instanceId || a[i].nodeId !== b[i].nodeId) {
      return false
    }
  }
  return true
}

function commonScenePathPrefix(a, b) {
  const out = []
  const max = Math.min(a.length, b.length)
  for (let i = 0; i < max; i++) {
    if (a[i].type !== b[i].type || a[i].instanceId !== b[i].instanceId || a[i].nodeId !== b[i].nodeId) {
      break
    }
    out.push({ type: a[i].type, instanceId: a[i].instanceId, nodeId: a[i].nodeId })
  }
  return out
}

function embedSegment(instanceId) {
  return { type: 'enterEmbed', instanceId, nodeId: null }
}

function describeScenePath(path) {
  if (path.length === 0) { return 'root' }
  let out = 'root'
  for (const seg of path) {
    if (seg.type === 'enterEmbed') { out += `/embed:${seg.instanceId ?? ''}` }
    else if (seg.type === 'enterPortal') { out += `/portal:${seg.nodeId ?? ''}` }
    else { out += `/${seg.type}:${seg.instanceId ?? ''}` }
  }
  return out
}

function emptyTargetDetail(type, nodeId) {
  return {
    type, nodeId,
    anchorId: null, projectId: null, volumeId: null, chapterId: null,
    rangeStart: null, rangeEnd: null, entityType: null, entityId: null, uri: null
  }
}

function buildTargetPathForHost(hostStarmapId, hostScenePath, itemRef) {
  const segments = itemRef.scenePath.slice(hostScenePath.length)
    .map(seg => ({ type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId }))
  if (itemRef.kind === 'embed') {
    segments.push(embedSegment(itemRef.itemId))
    return { starmapId: hostStarmapId, segments, target: emptyTargetDetail('starmap', null) }
  }
  return { starmapId: hostStarmapId, segments, target: emptyTargetDetail('node', itemRef.itemId) }
}

function isSameTargetPath(a, b) {
  if (a.starmapId !== b.starmapId) { return false }
  if (!isSameScenePath(a.segments, b.segments)) { return false }
  return a.target.type === b.target.type && a.target.nodeId === b.target.nodeId
}

function describeTargetPath(path) {
  return `${describeScenePath(path.segments)}#${path.target.type}:${path.target.nodeId ?? ''}`
}

// ══════════════════════════════════════════════════════════════
// 3. 命中与递归 Scene 树（StarMapGeometry.ets）
// ══════════════════════════════════════════════════════════════

function pointInEmbedCircle(rect, x, y) {
  const cx = rect.x + rect.width / 2
  const cy = rect.y + rect.height / 2
  const dx = x - cx
  const dy = y - cy
  return dx * dx + dy * dy <= (rect.width / 2) * (rect.width / 2)
}

function embedHitMetricsForScene(sceneScale, embedRadius) {
  const scale = sceneScale > 0 ? sceneScale : 1
  const limit = embedRadius > 0 ? embedRadius : 0
  return {
    titleHitHeight: Math.min(limit, EMBED_TITLE_HIT_HEIGHT / scale),
    borderHitWidth: Math.min(limit, EMBED_BORDER_HIT_WIDTH / scale)
  }
}

const EMBED_INPUT_NODE_ID_PREFIX = 'starmap_embed_input_'

function describeEmbedInputNodeId(scenePath, embedInstanceId) {
  return `${EMBED_INPUT_NODE_ID_PREFIX}${describeScenePath(scenePath)}_${embedInstanceId}`
}

function isEmbedInputNodeId(nodeId) {
  return nodeId.indexOf(EMBED_INPUT_NODE_ID_PREFIX) === 0
}

function describeSceneInputNodeId(scenePath) {
  return `${EMBED_INPUT_NODE_ID_PREFIX}${describeScenePath(scenePath)}`
}

function extractEmbedInstanceIdFromInputNodeId(scenePath, nodeId) {
  const prefix = `${EMBED_INPUT_NODE_ID_PREFIX}${describeScenePath(scenePath)}_`
  if (nodeId.indexOf(prefix) !== 0) { return null }
  const instanceId = nodeId.substring(prefix.length)
  return instanceId.length > 0 ? instanceId : null
}

function hitTestWithScene(rects, screenX, screenY, scenePath, embedInstanceIds, sceneScale) {
  for (let i = rects.length - 1; i >= 0; i--) {
    const r = rects[i]
    const isEmbed = embedInstanceIds.has(r.nodeId)
    if (isEmbed) {
      // #821：Embed 永远是那颗正圆，命中路径与 Deep Zoom 档位无关，没有矩形分支
      if (!pointInEmbedCircle(r, screenX, screenY)) { continue }
      const metrics = embedHitMetricsForScene(sceneScale, r.width / 2)
      if (screenY <= r.y + metrics.titleHitHeight) {
        return { scenePath, objectKind: 'embedTitle', objectId: r.nodeId, hitRegion: 'title' }
      }
      const cx = r.x + r.width / 2
      const cy = r.y + r.height / 2
      const outer = r.width / 2
      const inner = Math.max(0, outer - metrics.borderHitWidth)
      const dx = screenX - cx
      const dy = screenY - cy
      const dist = Math.sqrt(dx * dx + dy * dy)
      if (dist >= inner) {
        return { scenePath, objectKind: 'embedBorder', objectId: r.nodeId, hitRegion: 'border' }
      }
      return { scenePath, objectKind: 'embedInnerContent', objectId: r.nodeId, hitRegion: 'innerContent' }
    }
    if (screenX >= r.x && screenX <= r.x + r.width && screenY >= r.y && screenY <= r.y + r.height) {
      return { scenePath, objectKind: 'node', objectId: r.nodeId, hitRegion: 'body' }
    }
  }
  return null
}

function buildRecursiveSceneContext(root) {
  const children = new Map()
  for (const child of root.getChildEmbeds()) {
    const instanceId = describeLastEmbedInstanceId(child.scenePath)
    if (instanceId === '') { continue }
    children.set(instanceId, buildRecursiveSceneContext(child))
  }
  return {
    scenePath: cloneScenePath(root.scenePath),
    starmapId: root.starmapId,
    rects: root.rects,
    embedInstanceIds: root.embedInstanceIds,
    edges: root.edges,
    scale: root.scale,
    offsetX: root.offsetX,
    offsetY: root.offsetY,
    children
  }
}

function describeLastEmbedInstanceId(path) {
  if (path.length === 0) { return '' }
  const last = path[path.length - 1]
  if (last.type !== 'enterEmbed' || last.instanceId === null) { return '' }
  return last.instanceId
}

function findSceneContext(root, scenePath) {
  if (scenePath.length === 0) { return root }
  const seg = scenePath[0]
  if (seg.type !== 'enterEmbed' || seg.instanceId === null) { return null }
  const child = root.children.get(seg.instanceId)
  if (child === undefined) { return null }
  return findSceneContext(child, scenePath.slice(1))
}

function findEmbedRect(context, instanceId) {
  for (const rect of context.rects) {
    if (rect.nodeId === instanceId) { return rect }
  }
  return null
}

// 组件局部屏幕坐标只由**父层的显示几何**决定：盒子边长 = 直径 * 父 scale，
// 盒子左上角 = Embed 矩形原点 * 父 scale。
// 这里绝不能乘 child.scale——下一步的 screenToCanvas 已经乘过一次，两次会抵消，
// 那样"子图缩放对命中不可见"就又回来了（#816 评审第 3 条）。
function embedParentCanvasToChildLocal(parent, embedInstanceId, parentCanvasX, parentCanvasY) {
  const rect = findEmbedRect(parent, embedInstanceId)
  const originX = rect !== null ? rect.x : 0
  const originY = rect !== null ? rect.y : 0
  return { x: (parentCanvasX - originX) * parent.scale, y: (parentCanvasY - originY) * parent.scale }
}

function childLocalToParentCanvas(parent, embedInstanceId, childLocalX, childLocalY) {
  const rect = findEmbedRect(parent, embedInstanceId)
  const originX = rect !== null ? rect.x : 0
  const originY = rect !== null ? rect.y : 0
  const scale = parent.scale > 0 ? parent.scale : 1
  return { x: childLocalX / scale + originX, y: childLocalY / scale + originY }
}

const MAX_RECURSE_SCENE_DEPTH = 32

/** pointToSegmentDistance 的镜像（原在 StarMapGeometry.ets:330）。 */
function pointToSegmentDistance(px, py, x1, y1, x2, y2) {
  const dx = x2 - x1
  const dy = y2 - y1
  const segLenSq = dx * dx + dy * dy
  if (segLenSq === 0) {
    const dpx = px - x1
    const dpy = py - y1
    return Math.sqrt(dpx * dpx + dpy * dpy)
  }
  let t = ((px - x1) * dx + (py - y1) * dy) / segLenSq
  t = Math.max(0, Math.min(1, t))
  const projX = x1 + t * dx
  const projY = y1 + t * dy
  const distX = px - projX
  const distY = py - projY
  return Math.sqrt(distX * distX + distY * distY)
}

/** 边的命中容差，单位是**画布坐标**（调用方按本层 scale 换算，保证屏幕上粗细一致）。 */
const EDGE_HIT_TOLERANCE_VP = 10

/**
 * 复用 pointToSegmentDistance 的纯函数命中。
 *
 * 边的优先级**低于**节点和 Embed：resolveRecursiveHit 先测对象，只在整层都没命中时
 * 才调这个函数。否则端点贴着节点的边会把节点的点击抢走。
 * 距离相同时取先遍历到的那条，避免重叠的边互相争。
 */
function hitTestEdge(edges, x, y, tolerance) {
  let best = null
  let bestDistance = tolerance
  for (const edge of edges) {
    const distance = pointToSegmentDistance(x, y, edge.startX, edge.startY, edge.endX, edge.endY)
    // 严格小于：完全同距离时保留先遍历到的那条，和上面写的注释一致
    if (distance < bestDistance) { bestDistance = distance; best = edge }
  }
  return best
}

function resolveRecursiveHit(root, x, y) {
  let current = root
  let localX = x
  let localY = y
  const embedPath = []
  for (let depth = 0; depth <= MAX_RECURSE_SCENE_DEPTH; depth++) {
    const canvasPoint = screenToCanvas(localX, localY, current.scale, current.offsetX, current.offsetY)
    const hit = hitTestWithScene(current.rects, canvasPoint.x, canvasPoint.y, current.scenePath, current.embedInstanceIds, current.scale)
    if (hit === null) {
      const edgeTolerance = current.scale > 0 ? EDGE_HIT_TOLERANCE_VP / current.scale : EDGE_HIT_TOLERANCE_VP
      const edge = hitTestEdge(current.edges, canvasPoint.x, canvasPoint.y, edgeTolerance)
      if (edge !== null) {
        return {
          ownerScenePath: cloneScenePath(current.scenePath),
          ownerStarmapId: current.starmapId,
          target: {
            scenePath: cloneScenePath(current.scenePath),
            starmapId: current.starmapId,
            objectKind: 'edge',
            objectId: edge.edgeId,
            hitRegion: 'body'
          },
          embedPath: cloneScenePath(embedPath)
        }
      }
      return {
        ownerScenePath: cloneScenePath(current.scenePath),
        ownerStarmapId: current.starmapId,
        target: null,
        embedPath: cloneScenePath(embedPath)
      }
    }
    if (hit.objectKind !== 'embedInnerContent') {
      const kind = hit.objectKind === 'embedTitle' ? 'embed'
        : (hit.objectKind === 'embedBorder' ? 'embed' : 'node')
      return {
        ownerScenePath: cloneScenePath(current.scenePath),
        ownerStarmapId: current.starmapId,
        target: {
          scenePath: cloneScenePath(current.scenePath),
          starmapId: current.starmapId,
          objectKind: kind,
          objectId: hit.objectId,
          hitRegion: hit.hitRegion
        },
        embedPath: cloneScenePath(embedPath)
      }
    }
    const child = current.children.get(hit.objectId)
    if (child === undefined) {
      return {
        ownerScenePath: cloneScenePath(current.scenePath),
        ownerStarmapId: current.starmapId,
        target: {
          scenePath: cloneScenePath(current.scenePath),
          starmapId: current.starmapId,
          objectKind: 'embed',
          objectId: hit.objectId,
          hitRegion: 'innerContent'
        },
        embedPath: cloneScenePath(embedPath)
      }
    }
    const childLocal = embedParentCanvasToChildLocal(current, hit.objectId, canvasPoint.x, canvasPoint.y)
    embedPath.push(embedSegment(hit.objectId))
    current = child
    localX = childLocal.x
    localY = childLocal.y
  }
  return {
    ownerScenePath: cloneScenePath(current.scenePath),
    ownerStarmapId: current.starmapId,
    target: null,
    embedPath: cloneScenePath(embedPath)
  }
}

function convertPointToRoot(root, scenePath, x, y) {
  let localX = x
  let localY = y
  // 从最深的一段往外走：点本来就在最深 Scene 的局部坐标里，
  // 必须先退出最深那层，再退出它的父层——顺序反了就会拿错层的视口去套坐标。
  for (let index = scenePath.length - 1; index >= 0; index--) {
    const seg = scenePath[index]
    if (seg.type !== 'enterEmbed' || seg.instanceId === null) { break }
    const parent = index === 0 ? root : findSceneContext(root, scenePath.slice(0, index))
    if (parent === null) { break }
    const child = parent.children.get(seg.instanceId)
    if (child === undefined) { break }
    const parentCanvas = childLocalToParentCanvas(parent, seg.instanceId, localX, localY)
    const parentLocal = canvasToScreen(parentCanvas.x, parentCanvas.y, parent.scale, parent.offsetX, parent.offsetY)
    localX = parentLocal.x
    localY = parentLocal.y
  }
  return { x: localX, y: localY }
}

function resolveSceneLocalPoint(root, scenePath, x, y) {
  let current = root
  let localX = x
  let localY = y
  let index = 0
  while (index < scenePath.length) {
    const seg = scenePath[index]
    if (seg.type !== 'enterEmbed' || seg.instanceId === null) { break }
    const child = current.children.get(seg.instanceId)
    if (child === undefined) { break }
    const parentCanvas = screenToCanvas(localX, localY, current.scale, current.offsetX, current.offsetY)
    const childLocal = embedParentCanvasToChildLocal(current, seg.instanceId, parentCanvas.x, parentCanvas.y)
    current = child
    localX = childLocal.x
    localY = childLocal.y
    index++
  }
  return { x: localX, y: localY }
}

function resolveItemRefFromTargetPath(root, path) {
  if (path.segments.length === 0) {
    if (path.target.type === 'node' && path.target.nodeId !== null) {
      return { scenePath: [], starmapId: root.starmapId, kind: 'node', itemId: path.target.nodeId }
    }
    return null
  }
  if (path.target.type === 'node' && path.target.nodeId !== null) {
    // node 端点：segments 全是"走到所在 Scene"的穿越步骤，节点就在末段到达的那张图里
    const holder = findSceneContext(root, path.segments)
    if (holder === null) { return null }
    return { scenePath: cloneScenePath(path.segments), starmapId: holder.starmapId, kind: 'node', itemId: path.target.nodeId }
  }
  // Embed 端点：最后一段就是被引用的那个 Embed 自身，它属于**上一段**到达的 Scene
  const last = path.segments[path.segments.length - 1]
  if (last === undefined || last.type !== 'enterEmbed' || last.instanceId === null) { return null }
  const containerPath = path.segments.slice(0, path.segments.length - 1)
  const container = findSceneContext(root, containerPath)
  if (container === null) { return null }
  return { scenePath: cloneScenePath(containerPath), starmapId: container.starmapId, kind: 'embed', itemId: last.instanceId }
}

function planCrossLayerEdge(root, sourcePath, targetPath) {
  const from = resolveItemRefFromTargetPath(root, sourcePath)
  const to = resolveItemRefFromTargetPath(root, targetPath)
  if (from === null || to === null) { return null }
  if (from.kind === to.kind && from.itemId === to.itemId && isSameScenePath(from.scenePath, to.scenePath)) {
    return null
  }
  const hostScenePath = commonScenePathPrefix(from.scenePath, to.scenePath)
  const host = findSceneContext(root, hostScenePath)
  if (host === null) { return null }
  const hostStarmapId = host.starmapId
  const planFrom = buildTargetPathForHost(hostStarmapId, hostScenePath, from)
  const planTo = buildTargetPathForHost(hostStarmapId, hostScenePath, to)
  if (isSameTargetPath(planFrom, planTo)) { return null }
  return { hostScenePath, hostStarmapId, from: planFrom, to: planTo }
}

// ══════════════════════════════════════════════════════════════
// 4. 手势归属（StarMapGestureState.ets）
// ══════════════════════════════════════════════════════════════

function createGestureStateTracker() {
  const emptyState = () => ({
    mode: 'idle', activeItemId: '', activeItemKind: 'node', ownerScenePath: [],
    activeItemScenePath: null, panOwnerScenePath: null,
    connectOwnerScenePath: null, connectSourceScenePath: null,
    startPoint: { x: 0, y: 0 }, currentPoint: { x: 0, y: 0 },
    targetItemId: '', targetItemScenePath: null
  })
  const copyPath = p => p.map(seg => ({ type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId }))
  let state = emptyState()
  // #818 复审：双指 > 所有单指。pinch 认领之后晚到的单指 begin* 一律不许改写 mode。
  const isPinchingState = () => state.mode === 'pinch'
  return {
    reset() { state = emptyState() },
    beginPanCanvas(ownerScenePath, sx, sy) {
      if (isPinchingState()) { return }
      const n = emptyState()
      n.mode = 'panCanvas'
      n.ownerScenePath = copyPath(ownerScenePath)
      n.panOwnerScenePath = copyPath(ownerScenePath)
      n.startPoint = { x: sx, y: sy }
      n.currentPoint = { x: sx, y: sy }
      state = n
    },
    beginConnect(ownerScenePath, itemId, sx, sy) {
      if (isPinchingState()) { return }
      const n = emptyState()
      n.mode = 'connect'
      n.activeItemId = itemId
      n.activeItemKind = 'node'
      n.ownerScenePath = copyPath(ownerScenePath)
      n.activeItemScenePath = copyPath(ownerScenePath)
      n.connectOwnerScenePath = copyPath(ownerScenePath)
      n.connectSourceScenePath = copyPath(ownerScenePath)
      n.startPoint = { x: sx, y: sy }
      n.currentPoint = { x: sx, y: sy }
      state = n
    },
    beginNodeMenu(ownerScenePath, nodeId, sx, sy) {
      if (isPinchingState()) { return }
      const n = emptyState()
      n.mode = 'nodeMenu'
      n.activeItemId = nodeId
      n.activeItemKind = 'node'
      n.ownerScenePath = copyPath(ownerScenePath)
      n.activeItemScenePath = copyPath(ownerScenePath)
      n.startPoint = { x: sx, y: sy }
      n.currentPoint = { x: sx, y: sy }
      state = n
    },
    beginMoveNode(ownerScenePath, nodeId, sx, sy) {
      if (isPinchingState()) { return }
      const n = emptyState()
      n.mode = 'moveNode'
      n.activeItemId = nodeId
      n.activeItemKind = 'node'
      n.ownerScenePath = copyPath(ownerScenePath)
      n.activeItemScenePath = copyPath(ownerScenePath)
      n.startPoint = { x: sx, y: sy }
      n.currentPoint = { x: sx, y: sy }
      state = n
    },
    beginMoveEmbed(ownerScenePath, embedInstanceId, sx, sy) {
      if (isPinchingState()) { return }
      const n = emptyState()
      n.mode = 'moveEmbed'
      n.activeItemId = embedInstanceId
      n.activeItemKind = 'embed'
      n.ownerScenePath = copyPath(ownerScenePath)
      n.activeItemScenePath = copyPath(ownerScenePath)
      n.startPoint = { x: sx, y: sy }
      n.currentPoint = { x: sx, y: sy }
      state = n
    },
    // #818：双指没有归属层，只有"进行中"标志；相机在 StarMapScreen 上独占
    beginPinch(cx, cy) {
      const n = emptyState()
      n.mode = 'pinch'
      n.startPoint = { x: cx, y: cy }
      n.currentPoint = { x: cx, y: cy }
      state = n
    },
    endPinch() {
      if (state.mode !== 'pinch') { return }
      state = emptyState()
    },
    setTargetItemScenePath(scenePath) { state.targetItemScenePath = copyPath(scenePath) },
    isIdle() { return state.mode === 'idle' },
    isConnecting() { return state.mode === 'connect' },
    isPinching() { return state.mode === 'pinch' },
    isConnectOwnedByScene(scenePath) {
      if (state.connectOwnerScenePath === null) { return false }
      return isSameScenePath(state.connectOwnerScenePath, scenePath)
    },
    isPanOwnedByScene(scenePath) {
      if (state.panOwnerScenePath === null) { return false }
      return isSameScenePath(state.panOwnerScenePath, scenePath)
    },
    isActiveItemInScene(scenePath) {
      if (state.activeItemScenePath === null) { return false }
      return isSameScenePath(state.activeItemScenePath, scenePath)
    },
    isOwnedByScene(scenePath) {
      return isSameScenePath(state.ownerScenePath, scenePath)
    },
    // 只有 mode 和归属都匹配才清全局（对应 StarMapGestureState.clearIfOwnedBy）
    clearIfOwnedBy(expectedMode, scenePath) {
      if (state.mode !== expectedMode) { return false }
      if (!isSameScenePath(state.ownerScenePath, scenePath)) { return false }
      state = emptyState()
      return true
    },
    // 当前手势是否归属在指定 Scene 的子树里（含它自己）
    isOwnedBySceneSubtree(scenePath) {
      const owner = state.ownerScenePath
      if (owner.length < scenePath.length) { return false }
      for (let i = 0; i < scenePath.length; i++) {
        if (owner[i].type !== scenePath[i].type ||
          owner[i].instanceId !== scenePath[i].instanceId ||
          owner[i].nodeId !== scenePath[i].nodeId) {
          return false
        }
      }
      return true
    },
    getState() {
      const s = state
      return {
        mode: s.mode,
        activeItemId: s.activeItemId,
        activeItemKind: s.activeItemKind,
        ownerScenePath: copyPath(s.ownerScenePath),
        activeItemScenePath: s.activeItemScenePath !== null ? copyPath(s.activeItemScenePath) : null,
        panOwnerScenePath: s.panOwnerScenePath !== null ? copyPath(s.panOwnerScenePath) : null,
                connectOwnerScenePath: s.connectOwnerScenePath !== null ? copyPath(s.connectOwnerScenePath) : null,
        connectSourceScenePath: s.connectSourceScenePath !== null ? copyPath(s.connectSourceScenePath) : null,
        startPoint: { x: s.startPoint.x, y: s.startPoint.y },
        currentPoint: { x: s.currentPoint.x, y: s.currentPoint.y },
        targetItemId: s.targetItemId,
        targetItemScenePath: s.targetItemScenePath !== null ? copyPath(s.targetItemScenePath) : null
      }
    }
  }
}

// ══════════════════════════════════════════════════════════════
// 5. 全树唯一选中态（StarMapSelectionState.ets）
// ══════════════════════════════════════════════════════════════

function createSelectionState() {
  return {
    selectedScenePath: [],
    selectedKind: 'none',
    selectedItemId: '',
    select(scenePath, kind, itemId) {
      this.selectedScenePath = scenePath.map(seg => ({ type: seg.type, instanceId: seg.instanceId, nodeId: seg.nodeId }))
      this.selectedKind = kind
      this.selectedItemId = itemId
    },
    clear() {
      this.selectedScenePath = []
      this.selectedKind = 'none'
      this.selectedItemId = ''
    },
    hasSelection() { return this.selectedKind !== 'none' && this.selectedItemId.length > 0 },
    isSelected(scenePath, kind, itemId) {
      return this.hasSelection() && this.selectedKind === kind && this.selectedItemId === itemId &&
        isSameScenePath(this.selectedScenePath, scenePath)
    },
    isSceneSelected(scenePath) {
      return this.hasSelection() && isSameScenePath(this.selectedScenePath, scenePath)
    },
    describeSelection() {
      if (!this.hasSelection()) { return 'none' }
      return `${describeScenePath(this.selectedScenePath)}#${this.selectedKind}:${this.selectedItemId}`
    }
  }
}

// ══════════════════════════════════════════════════════════════
// 测试夹具：Scene 树
//
// 坐标关系（这是 #816 最容易搞错的一处，测试里显式写死）：
//   子 Scene 组件正好铺满 Embed 那个 200×200 的盒子，所以
//     子局部屏幕 = (父画布 − Embed 矩形原点) × 子scale + 子offset
//     子画布     = (子局部屏幕 − 子offset) / 子scale = 父画布 − Embed 矩形原点
//   也就是说子画布坐标就是"父画布坐标平移一个 Embed 原点"，两套坐标系
//   没有共同的数值范围——把父画布坐标直接塞进子视口换算，锚点会跑到子图外面。
//
// 布局（各层自己的画布坐标）：
//   根 Scene sm-root：scale 1，offset (0,0)
//     n-root     节点  (0,    0) 160×80
//     emb-a      Embed (400,  0) 200×200  → 子 Scene sm-a
//     emb-b      Embed (800,  0) 200×200  → 子 Scene sm-b
//     emb-slow   Embed (1200, 0) 200×200  → 子图未加载
//
//   子 Scene sm-a：scale 0.5，offset (10,10)
//     n-a1       节点  (0,  0)  80×40
//     emb-a1     Embed (20, 20) 160×160  → 孙 Scene sm-a1
//
//   孙 Scene sm-a1：scale 0.3，offset (0,0)
//     n-a1x      节点  (30, 100) 120×80
//
//   子 Scene sm-b：scale 0.5，offset (0,0)
//     n-b1       节点  (0, 0) 160×80
//
// 深层夹具（makeDeepTree）只有根 / sm-a / sm-a1 三层，
// 且把 emb-a1 放在子画布 (20,20)，保证从根画布点进去时仍落在 emb-a 圆内：
//   emb-a1 子画布 (20,20,160,160)，圆心 (100,100) → 父画布 (500,100) = emb-a 圆心
// ══════════════════════════════════════════════════════════════

const SEG_A = embedSegment('emb-a')
const SEG_A1 = embedSegment('emb-a1')
const SEG_B = embedSegment('emb-b')
const PATH_A = [SEG_A]
const PATH_A1 = [SEG_A, SEG_A1]
const PATH_B = [SEG_B]

function makeSceneSource(scenePath, starmapId, rects, embedIds, scale, offsetX, offsetY, children, edges) {
  return {
    scenePath, starmapId, rects,
    embedInstanceIds: new Set(embedIds),
    edges: edges || [],
    scale, offsetX, offsetY,
    getChildEmbeds() { return children }
  }
}

/** 三层夹具：根 / sm-a / sm-a1，另有 sm-b 和一个未加载的 emb-slow。 */
function makeTree() {
  const a1 = makeSceneSource(
    cloneScenePath(PATH_A1), 'sm-a1',
    [{ nodeId: 'n-a1x', x: 30, y: 100, width: 120, height: 80, radius: 16 }],
    [], 0.3, 0, 0, []
  )
  const a = makeSceneSource(
    cloneScenePath(PATH_A), 'sm-a',
    [
      { nodeId: 'n-a2', x: 0, y: 100, width: 80, height: 40, radius: 16 },
      { nodeId: 'n-a1', x: 0, y: 0, width: 80, height: 40, radius: 16 },
      { nodeId: 'emb-a1', x: 20, y: 20, width: 160, height: 160, radius: 80 }
    ],
    ['emb-a1'], 0.5, 10, 10, [a1]
  )
  const b = makeSceneSource(
    cloneScenePath(PATH_B), 'sm-b',
    [{ nodeId: 'n-b1', x: 0, y: 0, width: 160, height: 80, radius: 16 }],
    [], 0.5, 0, 0, []
  )
  return makeSceneSource(
    [], 'sm-root',
    [
      { nodeId: 'emb-slow', x: 1200, y: 0, width: 200, height: 200, radius: 100 },
      { nodeId: 'emb-b', x: 800, y: 0, width: 200, height: 200, radius: 100 },
      { nodeId: 'emb-a', x: 400, y: 0, width: 200, height: 200, radius: 100 },
      { nodeId: 'n-root', x: 0, y: 0, width: 160, height: 80, radius: 16 }
    ],
    ['emb-a', 'emb-b', 'emb-slow'], 1, 0, 0, [a, b]
  )
}

// ─── #816 评审回归需要的镜像（StarMapScene.ets / StarMapViewport.ets）───

/** 两指距离。搬掉系统 PinchGesture 之后，捏合幅度只能自己算。 */
function fingerDistance(a, b) {
  const dx = a.x - b.x
  const dy = a.y - b.y
  return Math.sqrt(dx * dx + dy * dy)
}

/**
 * 捏合比例 = 当前两指距离 ÷ 起手两指距离。
 *
 * 刻意不写成"当前 scale ÷ 起手 scale"：那条式子每帧自乘一次，
 * 两根手指张开 10% 画面会缩掉一大截，而且和手指实际开合完全脱钩。
 */
function computePinchRatio(baseDistance, fingerPoints) {
  if (!(baseDistance > 0) || fingerPoints.length < 2) { return 1 }
  return fingerDistance(fingerPoints[0], fingerPoints[1]) / baseDistance
}

/**
 * Scene 句柄的镜像（StarMapViewport.ets 的 StarMapSceneNode）。
 * 重点是 sync()：注册表里存的 scale/offset 不会因为 UI 改了 @Link 就自动更新，
 * 不主动 sync 的话，缩放一次之后所有递归命中都在拿过期视口算。
 */
function createSceneNode(scenePath, starmapId, opts) {
  const redraws = []
  const node = {
    scenePath, starmapId,
    rects: opts.rects || [],
    embedInstanceIds: new Set(opts.embedInstanceIds || []),
    edges: opts.edges || [],
    scale: opts.scale === undefined ? 1 : opts.scale,
    offsetX: opts.offsetX || 0,
    offsetY: opts.offsetY || 0,
    getChildEmbeds() { return opts.getChildEmbeds ? opts.getChildEmbeds() : [] },
    // #818：句柄里不再有任何缩放入口，scale/offset 只是根相机或本层局部 fit 的快照
    sync(rects, embedInstanceIds, edges, scale, offsetX, offsetY) {
      node.rects = rects
      node.embedInstanceIds = new Set(embedInstanceIds)
      node.edges = edges
      node.scale = scale
      node.offsetX = offsetX
      node.offsetY = offsetY
    },
    async createEdgeBetween(from, to) { return opts.createEdgeFn(from, to) },
    redrawEdges() { redraws.push(node.scenePath); return opts.redrawEdgesFn() }
  }
  return node
}

/**
 * StarMapSceneRegistry 的镜像。
 * redrawAllEdges 必须遍历**所有**已注册的 Scene：共享选中是整棵树唯一一份，
 * 选中态一变，每一层的边都可能要改（#816 复审第 8 条）。
 */
function createSceneRegistry() {
  const handles = new Map()
  return {
    register(handle) { handles.set(describeScenePath(handle.scenePath), handle) },
    unregister(scenePath) { handles.delete(describeScenePath(scenePath)) },
    find(scenePath) {
      const h = handles.get(describeScenePath(scenePath))
      return h === undefined ? null : h
    },
    getRoot() {
      const h = handles.get('root')
      return h === undefined ? null : h
    },
    size() { return handles.size },
    redrawAllEdges() { handles.forEach((handle) => { handle.redrawEdges() }) }
  }
}

/**
 * begin/updateUnifiedPinch 的镜像（ui/StarMapScene.ets，#818）。
 *
 * 双指手势只改一台全局相机。起手记下 camera 的比例、偏移和两指距离，
 * 之后每帧都从**起手那一帧**重算，所以 100 → 110 → 120 是 1.20 而不是 1.32。
 *
 * `touchCenter` 传进来只是为了演示"两指中心落在哪一层都一样"：
 * 这里完全不查递归命中，落在节点 / 子星图 / 二层子星图上算出的相机完全相同。
 */
function createGlobalCameraPinch(camera) {
  const st = {
    camera: { scale: camera.scale, offsetX: camera.offsetX, offsetY: camera.offsetY },
    baseScale: 0, baseOffsetX: 0, baseOffsetY: 0, baseDistance: 0, wasActive: false,
    begin(fingerPoints) {
      st.wasActive = true
      st.baseScale = st.camera.scale > 0 ? st.camera.scale : 1
      st.baseOffsetX = st.camera.offsetX
      st.baseOffsetY = st.camera.offsetY
      st.baseDistance = fingerPoints.length >= 2 ? fingerDistance(fingerPoints[0], fingerPoints[1]) : 0
    },
    update(fingerPoints, centerX, centerY) {
      if (fingerPoints.length < 2) { return }
      const dist = fingerDistance(fingerPoints[0], fingerPoints[1])
      st.camera = computeCameraPinch(
        { scale: st.baseScale, offsetX: st.baseOffsetX, offsetY: st.baseOffsetY },
        st.baseScale,
        st.baseDistance,
        dist,
        centerX,
        centerY
      )
    },
    end() { st.wasActive = false }
  }
  return st
}

/** 工具栏 +/− 的镜像（ui/StarMapScreen.ets）：围绕画布中心改同一台相机。 */
function zoomCameraAroundCenter(camera, centerX, centerY, nextScale) {
  return computeCameraZoomAround(camera, centerX, centerY, nextScale)
}

/**
 * StarMapScene 视口的镜像（ui/StarMapScene.ets）。
 *
 * 关键点（#821）：每个 Scene 都在**自己的 box 坐标空间**里渲染，节点 / Embed
 * 保持固定的本地布局尺寸，靠 `.scale(boxScale)` + 补偿过的 `.position()` 显示；
 * box 坐标 × parentScale() 才是累计屏幕坐标，`viewportScaleValue()` 就是这一份
 * （根层是 cameraScale，子层是 boxScale × parentScale，两者恒等）。
 *
 * 子 Scene 的 box 尺寸恒为本地 DEFAULT_EMBED_DIAMETER，所以 `localSceneWidth/Height`
 * 不再除以 parentScale，fit 也只在首次 / 内容变化时算一次（没有 syncFitToSceneSize）。
 */
function createSceneView(opts) {
  const v = {
    sceneDepth: opts.sceneDepth || 0,
    cameraScale: opts.cameraScale || 1,
    cameraOffsetX: opts.cameraOffsetX || 0,
    cameraOffsetY: opts.cameraOffsetY || 0,
    inheritedScale: opts.inheritedScale || 1,
    sceneWidth: opts.sceneWidth || 0,
    sceneHeight: opts.sceneHeight || 0,
    fitScale: 1,
    fitOffsetX: 0,
    fitOffsetY: 0,
    hasFittedView: false,
    isCameraScene() { return v.sceneDepth === 0 },
    parentScale() { return v.inheritedScale > 0 ? v.inheritedScale : 1 },
    localSceneWidth() { return v.sceneWidth },
    localSceneHeight() { return v.sceneHeight },
    boxScale() { return v.isCameraScene() ? v.cameraScale : v.fitScale },
    boxOffsetX() { return v.isCameraScene() ? v.cameraOffsetX : v.fitOffsetX },
    boxOffsetY() { return v.isCameraScene() ? v.cameraOffsetY : v.fitOffsetY },
    boxUnitVp() { return 1 / v.parentScale() },
    // ArkUI 绕组件中心缩放，所以布局位置要减掉一半的“长出来那截”，
    // 视觉左上角才等于 canvas × boxScale + boxOffset。
    boxPositionX(canvasX, baseWidth) {
      const s = v.boxScale()
      return canvasX * s + v.boxOffsetX() - baseWidth * (s - 1) / 2
    },
    boxPositionY(canvasY, baseHeight) {
      const s = v.boxScale()
      return canvasY * s + v.boxOffsetY() - baseHeight * (s - 1) / 2
    },
    boxPointToViewport(point) {
      return { x: point.x * v.parentScale(), y: point.y * v.parentScale() }
    },
    viewportScaleValue() {
      return v.isCameraScene() ? v.cameraScale : v.fitScale * v.parentScale()
    },
    viewportOffsetX() {
      return v.isCameraScene() ? v.cameraOffsetX : v.fitOffsetX * v.parentScale()
    },
    viewportOffsetY() {
      return v.isCameraScene() ? v.cameraOffsetY : v.fitOffsetY * v.parentScale()
    },
    fitView(rects) {
      v.hasFittedView = true
      v.applyFittedLocalViewport(rects, v.localSceneWidth(), v.localSceneHeight())
    },
    // 局部 fit 的唯一纯计算出口：首次 fit 与"父 Embed 基础尺寸真的变了"共用
    applyFittedLocalViewport(rects, localWidth, localHeight) {
      const bounds = computeContentBounds(rects)
      if (bounds === null) {
        v.fitScale = 1; v.fitOffsetX = 0; v.fitOffsetY = 0
        return
      }
      // 可用区 = 圆内接正方形 − 父圆要保留的标题/边框交互壳 − 留白。
      // 留白已经在 computeEmbedInnerContentSafeSide 里扣过，所以 padding 传 0。
      const available = computeEmbedInnerContentSafeSide(Math.min(localWidth, localHeight), EMBED_FIT_PADDING_VP)
      const fitted = computeFittedViewport(bounds, available, available, 0, localWidth, localHeight)
      v.fitScale = fitted.zoomScale
      v.fitOffsetX = fitted.offsetX
      v.fitOffsetY = fitted.offsetY
    },
    // #821：局部 box 尺寸恒为 DEFAULT_EMBED_DIAMETER，缩放走显示变换，
    // 不再有任何"尺寸真变了要重算 fit"的路径。
    // #818 复审：拖画布 = 拖全局相机。根 Scene 直接整份改相机；
    // 子 Scene 自己没有相机，只把目标 offset 回传给根（onCameraPanTo），
    // 它的 fitOffsetX/Y 不再出现在任何用户交互写路径里。
    //
    // PanGesture 的 event.offsetX/Y 是相对手势起点的累计位移，不是每帧增量，
    // 所以调用方必须在起手时记一份基准（panBaseCameraOffsetX/Y），
    // 用 基准 + 累计位移 算出目标 offset 传进来；这里只负责整份覆盖，不能再相加。
    applyCameraPanTo(offsetX, offsetY, onCameraPanTo) {
      if (v.isCameraScene()) {
        v.cameraOffsetX = offsetX
        v.cameraOffsetY = offsetY
        return
      }
      onCameraPanTo(offsetX, offsetY)
    },
    // #818 复审：子 Scene 里的对象不能被拖到圆外。写入前夹进圆内接正方形，
    // 夹完的这份坐标既进布局也存进 Core。
    clampItemToLocalSafeArea(x, y, width, height) {
      if (v.isCameraScene()) { return { x, y } }
      const localSceneSize = Math.min(v.localSceneWidth(), v.localSceneHeight())
      return clampItemToEmbedSafeArea(
        x, y, width, height,
        v.fitScale, v.fitOffsetX, v.fitOffsetY,
        localSceneSize, EMBED_FIT_PADDING_VP
      )
    },
    // #818 复审：新建也要守边界。光 clamp 不够 —— 空子星图首次 fitView 时没有内容，
    // fitScale 停在 1 且 hasFittedView 已为 true，之后 maybeFitView 不再跑，
    // 新对象会比当前安全区还大。所以新建成功后显式重算一次 local fit。
    // 根 Scene 不做（整屏，没有圆壳也没有 local fit）。
    // 重算的是 local fit 而不是 global camera，用户双指缩放好的视角不会被重置。
    refitAfterContentAdded(rects) {
      if (v.isCameraScene()) { return false }
      v.fitView(rects)
      return true
    },
    // 写进 Scene 注册表快照的就是累计有效比例 / 累计偏移
    sceneSourceSnapshot() {
      return { scale: v.viewportScaleValue(), offsetX: v.viewportOffsetX(), offsetY: v.viewportOffsetY() }
    }
  }
  return v
}

// ══════════════════════════════════════════════════════════════
// 断言工具
// ══════════════════════════════════════════════════════════════

let passed = 0
let failed = 0
function assert(cond, msg) {
  if (cond) { passed++; console.log('  PASS:', msg) }
  else { failed++; console.error('  FAIL:', msg) }
}
function eq(a, b) { return JSON.stringify(a) === JSON.stringify(b) }
function near(a, b, eps = 1e-6) { return Math.abs(a - b) <= eps }
function nearPt(p, x, y, eps = 1e-6) { return near(p.x, x, eps) && near(p.y, y, eps) }

// ══════════════════════════════════════════════════════════════
// 1. 内容包围盒
// ══════════════════════════════════════════════════════════════

console.log('1. computeContentBounds：并集包围盒，空列表返回 null')
{
  assert(computeContentBounds([]) === null, '空列表 → null（没有内容可适配）')
  const single = computeContentBounds([{ nodeId: 'a', x: 5, y: 7, width: 100, height: 50, radius: 0 }])
  assert(near(single.minX, 5) && near(single.minY, 7) && near(single.width, 100) && near(single.height, 50),
    '单个矩形 → 自身就是包围盒')
  const b = computeContentBounds([
    { nodeId: 'a', x: 0, y: 0, width: 100, height: 100, radius: 0 },
    { nodeId: 'b', x: -50, y: 20, width: 100, height: 10, radius: 0 },
    { nodeId: 'c', x: 10, y: -30, width: 20, height: 200, radius: 0 }
  ])
  assert(near(b.minX, -50) && near(b.minY, -30), '包围盒取各轴最小值（负坐标也算进去）')
  assert(near(b.maxX, 100) && near(b.maxY, 170), '包围盒取各轴最大值')
  assert(near(b.width, 150) && near(b.height, 200), 'width/height 是并集跨度')
}

// ══════════════════════════════════════════════════════════════
// 2. fitScale（布局适配）与全局相机
// ══════════════════════════════════════════════════════════════

console.log('2. fitScale 来自内容与可用区，不是"每层乘 0.6"')
{
  const available = DEFAULT_EMBED_DIAMETER * CIRCLE_INNER_SAFE_RATIO

  // 同样大的可用区，内容越大 fit 越小——写死的每层系数做不到这一点
  const small = computeContentBounds([{ nodeId: 'a', x: 0, y: 0, width: 100, height: 100, radius: 0 }])
  const large = computeContentBounds([{ nodeId: 'a', x: 0, y: 0, width: 1000, height: 1000, radius: 0 }])
  const smallFit = computeFitScale(small, available, available, EMBED_FIT_PADDING_VP)
  const largeFit = computeFitScale(large, available, available, EMBED_FIT_PADDING_VP)
  assert(smallFit > largeFit, `内容小的 fit 更大（${smallFit.toFixed(3)} > ${largeFit.toFixed(3)}）`)

  // 深度本身不影响 fit：两个子图内容一样大，fit 必须一样
  assert(near(computeFitScale(small, available, available, EMBED_FIT_PADDING_VP),
    computeFitScale(small, available, available, EMBED_FIT_PADDING_VP)),
  'fit 与嵌套深度无关（深度是路径，不是系数）')

  // 夹取
  const huge = computeContentBounds([{ nodeId: 'a', x: 0, y: 0, width: 1, height: 1, radius: 0 }])
  assert(near(computeFitScale(huge, available, available, EMBED_FIT_PADDING_VP), MAX_FIT_SCALE),
    '内容极小时 fit 夹在 MAX_FIT_SCALE，不被放大到糊掉')
  const vast = computeContentBounds([{ nodeId: 'a', x: 0, y: 0, width: 1e7, height: 1e7, radius: 0 }])
  assert(near(computeFitScale(vast, available, available, EMBED_FIT_PADDING_VP), MIN_FIT_SCALE),
    '内容极大时 fit 有下限，不会变成 0')

  // #818：fit 只是布局适配，不再和用户缩放相乘。子 Scene 的累计有效比例是
  // `fitScale × 祖先累计`，其中"用户缩放"只来自根那一台相机。
  const fit = 0.4
  const inherited = 2
  assert(near(fit * inherited, 0.8), '子 Scene 屏幕上的比例 = fitScale × 祖先累计（这里 0.4 × 2）')
  assert(!('userZoomScale' in { fitScale: fit }), '不存在 fitScale × userZoomScale 这条合成路径（#818 已删除）')
}

console.log('3. computeFittedViewport：缩放按可用区算，偏移按 Scene 自己的尺寸算')
{
  // 直径 200 的圆：可用区是内接正方形（约 141），但子 Scene 组件铺满整个圆（200×200）
  const available = 141
  const sceneSize = 200
  const bounds = computeContentBounds([
    { nodeId: 'a', x: 0, y: 0, width: 400, height: 100, radius: 0 },
    { nodeId: 'b', x: 400, y: 0, width: 400, height: 100, radius: 0 }
  ])
  const fitted = computeFittedViewport(
    bounds, available, available, EMBED_FIT_PADDING_VP, sceneSize, sceneSize
  )
  // 包围盒中心 (400, 50) 应落在 Scene 自己的中心，也就是圆心
  const center = canvasToScreen((bounds.minX + bounds.maxX) / 2, (bounds.minY + bounds.maxY) / 2,
    fitted.zoomScale, fitted.offsetX, fitted.offsetY)
  assert(nearPt(center, sceneSize / 2, sceneSize / 2, 1e-9),
    '适配后包围盒中心落在 Scene 中心（圆心），不是可用区中心')
  assert(!near(center.x, available / 2, 1e-6),
    '偏移不是拿内接正方形的中心当锚（那会让内容整体偏左上 ~70 而不是 100）')
  // 缩放比例仍然只看可用区，和 Scene 尺寸无关
  const sameScale = computeFitScale(bounds, available, available, EMBED_FIT_PADDING_VP)
  assert(near(fitted.zoomScale, sameScale), 'fitScale 仍由可用区决定')

  // 内容以 Scene 中心为心展开：fitScale 保证半宽不超过内接正方形的半边，
  // 所以内容仍然完整落在圆里（只是不再去贴内接正方形的边）
  const tl = canvasToScreen(bounds.minX, bounds.minY, fitted.zoomScale, fitted.offsetX, fitted.offsetY)
  const br = canvasToScreen(bounds.maxX, bounds.maxY, fitted.zoomScale, fitted.offsetX, fitted.offsetY)
  assert(tl.x >= sceneSize / 2 - available / 2 + 1e-9 && tl.y >= sceneSize / 2 - available / 2 + 1e-9,
    '内容左上角没有超出内接正方形（不会被 clip 裁掉）')
  assert(br.x <= sceneSize / 2 + available / 2 + 1e-9 && br.y <= sceneSize / 2 + available / 2 + 1e-9,
    '内容右下角也没有超出内接正方形')

  // 换 Scene 尺寸只挪偏移，不改比例：偏移跟着 Scene 中心走
  const wider = computeFittedViewport(bounds, available, available, EMBED_FIT_PADDING_VP, 300, 300)
  assert(near(wider.zoomScale, fitted.zoomScale), 'Scene 变大不改变 fitScale')
  const widerCenter = canvasToScreen((bounds.minX + bounds.maxX) / 2, (bounds.minY + bounds.maxY) / 2,
    wider.zoomScale, wider.offsetX, wider.offsetY)
  assert(nearPt(widerCenter, 150, 150, 1e-9), 'Scene 变大后内容跟着新的中心走')

  // 偏移公式本身
  const off = computeCenteredOffset(bounds, fitted.zoomScale, sceneSize, sceneSize)
  assert(near(off.x, sceneSize / 2 - 400 * fitted.zoomScale)
    && near(off.y, sceneSize / 2 - 50 * fitted.zoomScale),
  'computeCenteredOffset = Scene 中心 − 内容中心 × fitScale')
}

// ══════════════════════════════════════════════════════════════
// 4. 双指锚点
// ══════════════════════════════════════════════════════════════

console.log('4. 全局相机（#818）：只有一台，锚点钉在两指中心，落在哪一层都一样')
{
  const camera = { scale: 0.5, offsetX: 10, offsetY: 10 }
  const center = { x: 260, y: 60 }
  const anchorWorld = screenToCanvas(center.x, center.y, camera.scale, camera.offsetX, camera.offsetY)
  assert(nearPt(anchorWorld, 500, 100), '两指中心 → 根画布的世界坐标（相机自己的一套坐标系）')

  // 双指：起手 100 宽，捏到 200 宽
  const next = computeCameraPinch(camera, camera.scale, 100, 200, center.x, center.y)
  assert(near(next.scale, 1.0), '两指张开一倍 → 相机比例 ×2（0.5 → 1.0）')
  const still = canvasToScreen(anchorWorld.x, anchorWorld.y, next.scale, next.offsetX, next.offsetY)
  assert(nearPt(still, center.x, center.y, 1e-9), '捏合后世界锚点仍钉在两指中心')

  // 逐帧都从起手那一帧算，不能自乘
  const pinch = createGlobalCameraPinch({ scale: 1, offsetX: 0, offsetY: 0 })
  pinch.begin([{ x: 0, y: 0 }, { x: 100, y: 0 }])
  pinch.update([{ x: 0, y: 0 }, { x: 110, y: 0 }], 55, 0)
  pinch.update([{ x: 0, y: 0 }, { x: 120, y: 0 }], 60, 0)
  assert(near(pinch.camera.scale, 1.2), '两指 100 → 110 → 120 得到 1.20，不是 1.00×1.1×1.2 = 1.32')

  // 夹取
  assert(near(clampCameraScale(99), CAMERA_SCALE_MAX), '相机比例夹在上限 3')
  assert(near(clampCameraScale(0.01), CAMERA_SCALE_MIN), '相机比例有下限 0.3')
  assert(near(clampCameraScale(NaN), 1), '非法相机比例退回 1')
  assert(near(clampCameraScale(0), 1), '0 不是合法相机比例，退回 1（不能除零）')

  // 工具栏围绕画布中心缩放
  const bar = zoomCameraAroundCenter({ scale: 1, offsetX: 0, offsetY: 0 }, 500, 500, 2)
  assert(near(bar.scale, 2), '工具栏 + 把相机比例改到 2')
  const centerStill = canvasToScreen(500, 500, bar.scale, bar.offsetX, bar.offsetY)
  assert(nearPt(centerStill, 500, 500, 1e-9), '工具栏缩放也钉在画布中心')

  assert(near(sceneLocalToCanvas(10, 10, 0.5, 10, 10).x, 0), 'sceneLocalToCanvas / canvasToSceneLocal 互为逆运算')
}

console.log('4b. 双指落在哪一层都只改同一台全局相机（#818 退役方向）')
{
  const base = { scale: 1, offsetX: 30, offsetY: -20 }
  const ctx = buildRecursiveSceneContext(makeTree())

  // 两指中心分别落在根层节点上、子星图内部、二层子星图内部。
  // 递归命中仍然解析（选中/拖拽/连线要用），但捏合目标完全不看它。
  const centers = [{ x: 80, y: 40 }, { x: 500, y: 100 }]
  const depths = centers.map(c => resolveRecursiveHit(ctx, c.x, c.y).ownerScenePath.length)
  assert(depths.join(',') === '0,1',
    `两个两指中心分别命中第 ${depths.join(' / ')} 层（递归命中结果确实不同，缩放目标却不能跟着变）`)

  // 关键：不管命中哪一层，算出来的相机都是"以全局相机为基准"的那一个。
  // 偏移里只有全局相机的锚点 world 坐标和当前两指中心，没有任何层的信息。
  for (const center of centers) {
    const pinch = createGlobalCameraPinch(base)
    // 起手 100 宽，张开到 200 宽 → ×2
    pinch.begin([{ x: center.x - 50, y: center.y }, { x: center.x + 50, y: center.y }])
    pinch.update([{ x: center.x - 100, y: center.y }, { x: center.x + 100, y: center.y }], center.x, center.y)
    const anchorWorldX = (center.x - base.offsetX) / base.scale
    const anchorWorldY = (center.y - base.offsetY) / base.scale
    assert(near(pinch.camera.scale, 2),
      `两指中心 (${center.x},${center.y})：相机 ×2（缩的是整棵树，不是那一层）`)
    assert(near(pinch.camera.offsetX, center.x - anchorWorldX * 2) &&
      near(pinch.camera.offsetY, center.y - anchorWorldY * 2),
    '偏移只由全局相机锚点和两指中心决定，与命中层级无关')
  }

  // 对照：老实现会把偏移算成"按归属层视口反推的锚点"，落点完全不同
  const childScale = 0.4
  const legacyAnchor = screenToCanvas(500, 100, childScale, 0, 0)
  const correctAnchor = screenToCanvas(500, 100, base.scale, base.offsetX, base.offsetY)
  assert(!near(500 - legacyAnchor.x * 2, 500 - correctAnchor.x * 2),
    '用子层视口算锚点会得到另一个值（这正是 #818 要退役的"缩那一层"）')
}

// ══════════════════════════════════════════════════════════════
// 5. 递归命中
// ══════════════════════════════════════════════════════════════

console.log('5. resolveRecursiveHit：根层空白 / 根层节点 / Embed 标题 / 边框 / 内部')
{
  const ctx = buildRecursiveSceneContext(makeTree())

  // 根层空白（emb-a 左侧、n-root 上方）
  const blank = resolveRecursiveHit(ctx, 300, 150)
  assert(blank.target === null, '根层空白 → target 为 null')
  assert(eq(blank.ownerScenePath, []), '空白归属根 Scene')
  assert(blank.ownerStarmapId === 'sm-root', '归属 Scene 的 starmapId 是 sm-root')

  // 根层节点
  const node = resolveRecursiveHit(ctx, 50, 40)
  assert(node.target.objectKind === 'node' && node.target.objectId === 'n-root', '根层节点命中')
  assert(node.target.hitRegion === 'body' && eq(node.target.scenePath, []), '节点命中区域 body，路径是根')
  assert(eq(node.ownerScenePath, []), '根层节点归属根 Scene，不下沉')

  // Embed 标题：圆内顶部 24vp
  const title = resolveRecursiveHit(ctx, 500, 20)
  assert(title.target.objectKind === 'embed' && title.target.objectId === 'emb-a', 'Embed 标题命中')
  assert(title.target.hitRegion === 'title', '标题区域 → title')
  assert(eq(title.ownerScenePath, []), '命中 Embed 标题归属**父** Scene（Embed 属于父层）')
  assert(eq(title.embedPath, []), '停在标题不上报 embed 路径')

  // Embed 边框：圆心右侧 95vp（外侧 12vp 圆环）
  const border = resolveRecursiveHit(ctx, 595, 100)
  assert(border.target.objectKind === 'embed' && border.target.hitRegion === 'border', 'Embed 圆环命中')
  assert(eq(border.ownerScenePath, []), '命中圆环归属父 Scene')

  // Embed 内部 → 递归进子 Scene
  // 根局部 (460,60) → 子组件局部 (60,60) → sm-a 画布 (100,100) = emb-a1 圆心
  //   → sm-a1 局部 (40,40) → sm-a1 画布 (133.3,133.3) → 命中 n-a1x(30,100,120,80)
  const deep = resolveRecursiveHit(ctx, 460, 60)
  assert(eq(deep.ownerScenePath, PATH_A1), 'Embed 内部且子图已加载 → 归属一路下沉到第三层')
  assert(eq(deep.embedPath, [SEG_A, SEG_A1]), 'embedPath 完整记录经过的两段 Embed')
  assert(deep.target !== null && deep.target.objectKind === 'node' && deep.target.objectId === 'n-a1x',
    '一路解析到第三层的真实节点 n-a1x（不是选 emb-a1）')
  assert(deep.target.starmapId === 'sm-a1', '节点带的是第三层 Scene 的 starmapId')

  // 第二层 Embed 标题：根局部 (460,30) → sm-a 画布 (100,40) = emb-a1 顶部 24vp 内
  const secondTitle = resolveRecursiveHit(ctx, 460, 30)
  assert(secondTitle.target.objectKind === 'embed' && secondTitle.target.objectId === 'emb-a1',
    '第二层 Embed 标题命中的是那个 Embed')
  assert(secondTitle.target.hitRegion === 'title' && eq(secondTitle.ownerScenePath, PATH_A),
    '第二层 Embed 归属它的父 Scene（第二层），不下沉进第三层')
  assert(secondTitle.target.starmapId === 'sm-a', '第二层 Embed 带的是第二层 Scene 的 starmapId')

  // 子图未加载 → 停在父层，目标仍是 Embed
  const slow = resolveRecursiveHit(ctx, 1300, 100)
  assert(slow.target.objectKind === 'embed' && slow.target.objectId === 'emb-slow',
    '子图未加载时目标是这个 Embed（点它等于选中子星图）')
  assert(slow.target.hitRegion === 'innerContent' && eq(slow.ownerScenePath, []),
    '未加载的 Embed 内部归属父 Scene，不退化成选空白')
}

console.log('6. resolveRecursiveHit：空白与嵌套归属')
{
  const ctx = buildRecursiveSceneContext(makeTree())

  // 子 Scene 空白：根局部 (460,105)
  //   → sm-a 局部 (60,105) → sm-a 画布 (100,190)：离 emb-a1 圆心 (100,100) 距离 90 > 80 → 在圆外
  //   → 也不在 n-a1(0,0,80,40) / n-a2(0,100,80,40)（两者 x 都只到 80）→ sm-a 空白
  const childBlank = resolveRecursiveHit(ctx, 460, 105)
  assert(childBlank.target === null, '子 Scene 内的空白 → target 为 null')
  assert(eq(childBlank.ownerScenePath, PATH_A), '子图空白归属子 Scene（手势属于子图，不属于父图）')
  assert(eq(childBlank.embedPath, [SEG_A]), 'embedPath 记录已经穿过的那一段')

  // 深层节点：根局部 (460,60)
  //   → sm-a 画布 (100,100) = emb-a1 圆心 → 下沉
  //   → sm-a1 画布 (133.3,133.3) → 命中 n-a1x(30,100,120,80)
  const deep = resolveRecursiveHit(ctx, 460, 60)
  assert(eq(deep.ownerScenePath, PATH_A1), '两层嵌套：两指中心一路下沉到第三层 Scene')
  assert(deep.target !== null && deep.target.objectKind === 'node' && deep.target.objectId === 'n-a1x',
    '三层嵌套命中的是最深的真实节点，不是中间的 emb-a1')
  assert(deep.target.starmapId === 'sm-a1', '深层节点带第三层的 starmapId')
  assert(eq(deep.embedPath, [SEG_A, SEG_A1]), 'embedPath 完整记录经过的两段 Embed')
  assert(eq(deep.target.scenePath, PATH_A1), 'target 自带 scenePath，身份不含糊')

  // 深层空白：根局部 (460,80)
  //   → sm-a 局部 (60,80) → sm-a 画布 (100,140)：离 emb-a1 圆心 40，
  //     既不在标题带（画布 y 140 > 20 + 48）也不在圆环（40 < 56）→ 下沉
  //   → sm-a1 画布 (133.3,200)：n-a1x 的 y 到 180 为止 → 空白
  //
  // 这里的点必须重新选：旧点 (450,38) 落在 emb-a1 圆内顶部 18vp，
  // 正是屏幕侧标题热区（24vp）该覆盖的地方。以前按固定 24 画布单位判，
  // 缩进一层之后只剩屏幕上 12vp，于是把它误判成 innerContent 继续下沉——
  // 屏幕上明明点在标题上，却进了子图。判定改成按屏幕换算之后它就该算标题。
  const deepBlank = resolveRecursiveHit(ctx, 460, 80)
  assert(deepBlank.target === null, '第三层空白 → target 为 null（不误判成选中 emb-a1）')
  assert(eq(deepBlank.ownerScenePath, PATH_A1), '深层空白归属第三层 Scene')

  // 屏幕侧热区在缩进之后仍然是 24vp：同一条边上"离圆顶 12vp"和"离圆顶 18vp"必须分属
  // title 和 innerContent 两类，而不是都随 local fit 一起缩水。
  const smTitle = resolveRecursiveHit(ctx, 460, 30)
  assert(smTitle.target !== null && smTitle.target.objectKind === 'embed' &&
    smTitle.target.objectId === 'emb-a1' && smTitle.target.hitRegion === 'title',
  '第 2 层 Embed 的屏幕 12vp 处仍判 title（不是随 fit 缩成几 vp 的 innerContent）')
  assert(eq(smTitle.ownerScenePath, PATH_A), '命中标题就停在第 2 层，不下沉进子图')
  const smInner = resolveRecursiveHit(ctx, 480, 60)
  assert(smInner.target === null || smInner.target.objectId !== 'emb-a1',
  '离圆顶 36vp 处是 innerContent，不会停在第 2 层把它误判成选中 emb-a1')
  assert(eq(smInner.ownerScenePath, PATH_A1), 'innerContent 一路下沉到第三层，归属跟着走')
  const smBorder = resolveRecursiveHit(ctx, 494, 60)
  assert(smBorder.target !== null && smBorder.target.objectKind === 'embed' &&
    smBorder.target.objectId === 'emb-a1' && smBorder.target.hitRegion === 'border',
  '离圆侧边 12vp 处判 border，屏幕宽度不随层数缩水')

  // 第 3 条复审的关键回归：子 Scene 自己的缩放必须影响命中。
  // 把 sm-a1 的比例从 0.3 改成 0.15，同一个屏幕点的落点就变了——
  // 如果换算里把子视口算了两遍（老公式），改这个值对结果毫无影响。
  const shrunk = makeTree()
  const a1Node = shrunk.getChildEmbeds()[0].getChildEmbeds()[0]
  a1Node.scale = 0.15
  const shrunkCtx = buildRecursiveSceneContext(shrunk)
  const shrunkHit = resolveRecursiveHit(shrunkCtx, 460, 60)
  assert(eq(shrunkHit.ownerScenePath, PATH_A1), '子 Scene 缩放后归属层不变（仍在第三层）')
  assert(shrunkHit.target === null || shrunkHit.target.objectId !== 'n-a1x',
    '子 Scene 自己的比例变化会改变命中结果（老公式下这里永远命中 n-a1x）')
}

// ══════════════════════════════════════════════════════════════
// 6. 坐标换算
// ══════════════════════════════════════════════════════════════

console.log('7. Scene 间坐标换算：扣掉 Embed 矩形原点，且上下互为逆运算')
{
  const ctx = buildRecursiveSceneContext(makeTree())

  // 根画布 (500,100) → 子组件局部 (100,100)：扣 Embed 矩形原点 (400,0) 后乘**父** scale
  const asChildLocal = embedParentCanvasToChildLocal(ctx, 'emb-a', 500, 100)
  assert(nearPt(asChildLocal, 100, 100), '父画布 (500,100) → 子组件局部 (100,100)')

  // 第 3 条复审的关键回归：这里绝不能乘子 Scene 的视口。
  // 下一步的 screenToCanvas 已经乘过一次子视口，两次相乘等于没乘，
  // 那样"子图缩放对命中不可见"就又回来了。
  const naive = canvasToScreen(100, 100, 0.5, 10, 10)
  assert(!nearPt(naive, 100, 100),
    '套上子视口会得到 (60,60) 而不是 (100,100) —— 子视口只能在下一步生效一次')
  assert(nearPt(childLocalToParentCanvas(ctx, 'emb-a', 100, 100), 500, 100),
    '子组件局部 (100,100) → 回父画布 (500,100)，两个方向互为逆运算')

  // resolveSceneLocalPoint（根 → 指定层）与 convertPointToRoot（指定层 → 根）互为逆运算
  for (const path of [PATH_A, PATH_B, PATH_A1]) {
    const local = resolveSceneLocalPoint(ctx, path, 500, 100)
    const back = convertPointToRoot(ctx, path, local.x, local.y)
    assert(nearPt(back, 500, 100), `根 ↔ ${describeScenePath(path)} 换算互为逆运算`)
  }
  assert(nearPt(resolveSceneLocalPoint(ctx, PATH_A, 500, 100), 100, 100), '根 → 第二层局部 (100,100)')
  assert(nearPt(resolveSceneLocalPoint(ctx, PATH_A1, 500, 100), 80, 80), '根 → 第三层局部 (80,80)')

  // 递归命中的下探必须和 resolveSceneLocalPoint 是同一份真相，不是两套算法
  const hit = resolveRecursiveHit(ctx, 460, 60)
  const viaHelper = resolveSceneLocalPoint(ctx, hit.ownerScenePath, 460, 60)
  const childCtx = findSceneContext(ctx, hit.ownerScenePath)
  const viaHitCanvas = screenToCanvas(viaHelper.x, viaHelper.y, childCtx.scale, childCtx.offsetX, childCtx.offsetY)
  assert(nearPt(viaHitCanvas, 100, 100, 1e-6) || nearPt(viaHitCanvas, 133.3333, 133.3333, 1e-3),
    '递归命中下探用的换算 == resolveSceneLocalPoint 再退画布')

  // 回归：convertPointToRoot 必须从最深一段往外走。
  // 曾经从根往里走，于是把第三层的局部坐标当成第二层的，两层以上全错。
  const forward = resolveSceneLocalPoint(ctx, PATH_A1, 460, 60)
  assert(nearPt(convertPointToRoot(ctx, PATH_A1, forward.x, forward.y), 460, 60),
    '两层以上的点也能原样换算回根（convertPointToRoot 由内向外）')

  // 未加载的 Scene：按已走到的最深层返回，不抛错
  const missing = resolveSceneLocalPoint(ctx, [embedSegment('emb-slow')], 1300, 100)
  assert(nearPt(missing, 1300, 100), 'Scene 未注册时原样返回该点（不抛错）')
}

// ══════════════════════════════════════════════════════════════
console.log('8. 端点路径反解：node 落在哪一层由 target.type 决定，不能一律砍掉最后一段')
{
  const ctx = buildRecursiveSceneContext(makeTree())

  // node 端点：segments 是"走到所在 Scene"的穿越步骤，节点就在末段到达的那张图里
  const nodeInA = buildTargetPathForHost('sm-root', [],
    { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })
  const refNode = resolveItemRefFromTargetPath(ctx, nodeInA)
  assert(refNode !== null && eq(refNode.scenePath, PATH_A) && refNode.starmapId === 'sm-a',
    'Embed 里的 node 解析回它自己所在的 Scene，而不是父 Scene')

  const nodeInA1 = buildTargetPathForHost('sm-root', [],
    { scenePath: PATH_A1, starmapId: 'sm-a1', kind: 'node', itemId: 'n-a1x' })
  const refDeep = resolveItemRefFromTargetPath(ctx, nodeInA1)
  assert(refDeep !== null && eq(refDeep.scenePath, PATH_A1) && refDeep.starmapId === 'sm-a1',
    '第三层的 node 解析回第三层 Scene')

  // Embed 端点：最后一段是被引用的 Embed 自身，它属于上一段到达的 Scene
  const embedInA = buildTargetPathForHost('sm-root', [],
    { scenePath: PATH_A, starmapId: 'sm-a', kind: 'embed', itemId: 'emb-a1' })
  const refEmbed = resolveItemRefFromTargetPath(ctx, embedInA)
  assert(refEmbed !== null && refEmbed.kind === 'embed' && refEmbed.itemId === 'emb-a1'
    && eq(refEmbed.scenePath, PATH_A) && refEmbed.starmapId === 'sm-a',
    'Embed 端点落在容纳它的 Scene 上')

  // 回归：两层 node 曾被砍掉最后一段，误判成第二层 Scene，
  // 于是"第二层节点 → 第三层节点"会被算成同层，最近公共祖先整个错一层
  const plan = planCrossLayerEdge(ctx, nodeInA, nodeInA1)
  assert(plan !== null && eq(plan.hostScenePath, PATH_A),
    '第二层 → 第三层的最近公共祖先是第二层，不是根也不是第三层')
}

// ══════════════════════════════════════════════════════════════
// 7. 手势归属
// ══════════════════════════════════════════════════════════════

console.log('9. 双指不再有归属（#818）：只有一个进行中标志，相机在根上')
{
  const tracker = createGestureStateTracker()
  assert(tracker.isPinching() === false, '初始没有双指')

  tracker.beginPinch(460, 60)
  assert(tracker.isPinching(), 'beginPinch 后处于双指缩放中')
  const st = tracker.getState()
  assert(st.mode === 'pinch', 'mode = pinch')
  assert(eq(st.ownerScenePath, []), '双指归属根 Scene（相机在根，不存在"归属层"这回事）')
  assert(!('pinchOwnerScenePath' in st), '状态里没有 pinchOwnerScenePath（#818 已删）')
  assert(tracker.isIdle() === false, '双指中不是 idle')
  assert(!('isPinchOwnedByScene' in tracker) && !('canClaimPinch' in tracker),
    'isPinchOwnedByScene / canClaimPinch 已删除：任何一层都不能声称自己拥有缩放目标')

  tracker.endPinch()
  assert(tracker.isPinching() === false, '双指抬起后不再处于缩放中')
  tracker.endPinch()
  assert(tracker.isIdle(), '重复 endPinch 不会把别的 mode 清掉')
}

console.log('10. connect 归属与起点路径各记各的')
{
  const tracker = createGestureStateTracker()
  tracker.beginConnect(PATH_A, 'n-a1', 100, 100)
  assert(tracker.isConnectOwnedByScene(PATH_A), 'connect 归属发起层')
  assert(tracker.isConnectOwnedByScene([]) === false, '父层不归属这次 connect')
  assert(tracker.getState().connectSourceScenePath !== null &&
    eq(tracker.getState().connectSourceScenePath, PATH_A),
  'connect 起点 Scene 路径单独记（跨层连线靠它求最近公共祖先）')
  tracker.setTargetItemScenePath([])
  assert(eq(tracker.getState().targetItemScenePath, []), '终点 Scene 路径也单独记（终点可能在父层）')
  tracker.reset()
  assert(tracker.isConnecting() === false && tracker.isPinching() === false, 'reset 清空全部归属')
}

// ══════════════════════════════════════════════════════════════
// 8. 全树唯一选中态
// ══════════════════════════════════════════════════════════════

console.log('11. 全树唯一选中态：身份含 scenePath，选子节点立刻让父 Embed 失选')
{
  const sel = createSelectionState()
  assert(sel.hasSelection() === false && sel.describeSelection() === 'none', '初始无选中')

  // 选中父层 Embed
  sel.select([], 'embed', 'emb-a')
  assert(sel.isSelected([], 'embed', 'emb-a'), '父层 Embed 选中')
  assert(sel.isSceneSelected([]), '父 Scene 是选中所在层')
  assert(sel.describeSelection() === 'root#embed:emb-a', '可读描述带 scenePath')

  // 再选中子节点 → 父 Embed 立刻失选
  sel.select(PATH_A, 'node', 'n-a1')
  assert(sel.isSelected([], 'embed', 'emb-a') === false, '选中子节点后父 Embed 立即失选（全树只有一份选中态）')
  assert(sel.isSelected(PATH_A, 'node', 'n-a1'), '子节点选中')
  assert(sel.isSceneSelected([]) === false, '父 Scene 不再是选中所在层')

  // 选中第二层 Embed 标题 → 只选中那一个 Embed
  sel.select(PATH_A, 'embed', 'emb-a1')
  assert(sel.isSelected(PATH_A, 'embed', 'emb-a1'), '第二层 Embed 选中')
  assert(sel.isSelected([], 'embed', 'emb-a') === false, '父层 Embed 没被连带选中')
  assert(sel.isSelected(PATH_A, 'node', 'n-a1') === false, '同层其他对象没被连带选中')

  // 同名 item 在不同层不互相命中
  sel.select(PATH_B, 'node', 'same-id')
  assert(sel.isSelected(PATH_B, 'node', 'same-id'), 'B 层同名节点选中')
  assert(sel.isSelected(PATH_A, 'node', 'same-id') === false, 'A 层同名节点不算选中（身份含 scenePath）')
  assert(sel.isSelected([], 'node', 'same-id') === false, '根层同名节点也不算选中')

  // kind 也是身份的一部分
  assert(sel.isSelected(PATH_B, 'embed', 'same-id') === false, '同名不同 kind 不互相命中')

  sel.clear()
  assert(sel.hasSelection() === false && sel.isSceneSelected([]) === false, 'clear 清空选中')
}

// ══════════════════════════════════════════════════════════════
// 9. 跨层连线
// ══════════════════════════════════════════════════════════════

console.log('12. planCrossLayerEdge：父 Node → 子 Node，宿主是父 Scene')
{
  const ctx = buildRecursiveSceneContext(makeTree())
  const from = buildTargetPathForHost('sm-root', [], { scenePath: [], starmapId: 'sm-root', kind: 'node', itemId: 'n-root' })
  const to = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })
  const plan = planCrossLayerEdge(ctx, from, to)
  assert(plan !== null, '能规划出方案')
  assert(eq(plan.hostScenePath, []), '宿主 = 根 Scene（最近公共祖先）')
  assert(plan.hostStarmapId === 'sm-root', '宿主 starmapId = sm-root，就是 addStarMapEdge 的 starmapId')
  assert(eq(plan.from.segments, []) && plan.from.target.nodeId === 'n-root', '起点 segments 为空、target 是根节点')
  assert(eq(plan.to.segments, [SEG_A]) && plan.to.target.nodeId === 'n-a1',
    '终点 segments = [enterEmbed(emb-a)]，target 是子节点')
  assert(plan.to.target.type === 'node' && plan.to.target.anchorId === null && plan.to.target.uri === null,
    'target detail 全字段显式存在（Core serde 要求键存在）')
}

console.log('13. planCrossLayerEdge：子 Node → 父 Node，方向对称')
{
  const ctx = buildRecursiveSceneContext(makeTree())
  const from = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })
  const to = buildTargetPathForHost('sm-root', [], { scenePath: [], starmapId: 'sm-root', kind: 'node', itemId: 'n-root' })
  const plan = planCrossLayerEdge(ctx, from, to)
  assert(plan !== null && eq(plan.hostScenePath, []), '宿主仍是根 Scene')
  assert(eq(plan.from.segments, [SEG_A]) && plan.from.target.nodeId === 'n-a1', '起点带 enterEmbed 段')
  assert(eq(plan.to.segments, []) && plan.to.target.nodeId === 'n-root', '终点是根节点，无 embed 段')
}

console.log('14. planCrossLayerEdge：同一 Embed 里的两个子 Node，宿主下沉到该 Embed')
{
  const ctx = buildRecursiveSceneContext(makeTree())
  const from = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })
  const to = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a2' })
  const plan = planCrossLayerEdge(ctx, from, to)
  assert(plan !== null, '能规划出方案')
  assert(eq(plan.hostScenePath, PATH_A), '宿主 = 该 Embed 的 Scene，不是根')
  assert(plan.hostStarmapId === 'sm-a', '宿主 starmapId = sm-a（边写进子星图自己的图）')
  assert(eq(plan.from.segments, []) && eq(plan.to.segments, []),
    '两端相对宿主都没有 embed 段（同层连线退化结果）')
  assert(plan.from.target.nodeId === 'n-a1' && plan.to.target.nodeId === 'n-a2', '两端 target 是各自节点')
}

console.log('15. planCrossLayerEdge：两个不同 Embed 里的 Node，宿主是根，各带一段')
{
  const ctx = buildRecursiveSceneContext(makeTree())
  const from = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })
  const to = buildTargetPathForHost('sm-root', [], { scenePath: PATH_B, starmapId: 'sm-b', kind: 'node', itemId: 'n-b1' })
  const plan = planCrossLayerEdge(ctx, from, to)
  assert(plan !== null && eq(plan.hostScenePath, []), '最近公共祖先是根 Scene')
  assert(plan.hostStarmapId === 'sm-root', '边写进根星图')
  assert(eq(plan.from.segments, [SEG_A]), '起点带 emb-a 段')
  assert(eq(plan.to.segments, [SEG_B]), '终点带 emb-b 段')
}

console.log('16. planCrossLayerEdge：子 Node → 更深一层的子 Embed / Node')
{
  const ctx = buildRecursiveSceneContext(makeTree())
  // 子 Node → 第二层 Embed（Embed 自身作为端点：target.type = starmap）
  const from = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })
  const to = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'embed', itemId: 'emb-a1' })
  const planA = planCrossLayerEdge(ctx, from, to)
  assert(planA !== null && eq(planA.hostScenePath, PATH_A), '子 Node ↔ 同层更深 Embed：宿主是该 Embed 的 Scene')
  assert(eq(planA.from.segments, []) && planA.from.target.type === 'node', '起点是同层节点')
  assert(eq(planA.to.segments, [SEG_A1]) && planA.to.target.type === 'starmap',
    '终点是第二层 Embed：segments 追加一段 enterEmbed，target.type = starmap')

  // 子 Node → 第三层 Node
  const deepCtx = buildRecursiveSceneContext(makeTree())
  const deepFrom = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })
  const deepTo = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A1, starmapId: 'sm-a1', kind: 'node', itemId: 'n-a1x' })
  const planB = planCrossLayerEdge(deepCtx, deepFrom, deepTo)
  assert(planB !== null, '子 Node → 第三层 Node 能规划出方案')
  assert(eq(planB.hostScenePath, PATH_A), '宿主 = 第二层 Scene（两者的最近公共祖先）')
  assert(planB.hostStarmapId === 'sm-a', '边写进第二层星图')
  assert(eq(planB.from.segments, []) && planB.from.target.nodeId === 'n-a1', '起点是第二层节点，无 embed 段')
  assert(eq(planB.to.segments, [SEG_A1]) && planB.to.target.nodeId === 'n-a1x', '终点带 emb-a1 段，target 是第三层节点')
}

console.log('17. planCrossLayerEdge：自环与不可解析的输入')
{
  const ctx = buildRecursiveSceneContext(makeTree())
  const self = buildTargetPathForHost('sm-root', [], { scenePath: [], starmapId: 'sm-root', kind: 'node', itemId: 'n-root' })
  assert(planCrossLayerEdge(ctx, self, self) === null, '自环 → null（不产生自环边）')

  // 同 Scene 内两个不同节点：合法，和原来一样落在本层
  const a = buildTargetPathForHost('sm-root', [], { scenePath: [], starmapId: 'sm-root', kind: 'node', itemId: 'n-root' })
  const b = buildTargetPathForHost('sm-root', [], { scenePath: [], starmapId: 'sm-root', kind: 'node', itemId: 'n-root2' })
  const same = planCrossLayerEdge(ctx, a, b)
  assert(same !== null && eq(same.hostScenePath, []) && eq(same.from.segments, []) && eq(same.to.segments, []),
    '同层两个节点：宿主是本层、两端 segments 都为空（与改动前一致）')

  // 同 Scene 的 node → embed：embed 端点带一段 enterEmbed
  const toEmbed = buildTargetPathForHost('sm-root', [], { scenePath: [], starmapId: 'sm-root', kind: 'embed', itemId: 'emb-a' })
  const nodeToEmbed = planCrossLayerEdge(ctx, a, toEmbed)
  assert(nodeToEmbed !== null && eq(nodeToEmbed.to.segments, [SEG_A]) && nodeToEmbed.to.target.type === 'starmap',
    '同层 node → embed：embed 端点是一段 enterEmbed + target.type=starmap')

  // enterPortal 段不支持跨层连线
  const portal = {
    starmapId: 'sm-root',
    segments: [{ type: 'enterPortal', instanceId: null, nodeId: 'p1' }],
    target: emptyTargetDetail('node', 'n-x')
  }
  assert(planCrossLayerEdge(ctx, a, portal) === null, 'enterPortal 不是 Embed 容器 → null，不猜')

  // Scene 未加载
  const unloaded = buildTargetPathForHost('sm-root', [],
    { scenePath: [embedSegment('emb-slow')], starmapId: 'sm-slow', kind: 'node', itemId: 'n-slow' })
  assert(planCrossLayerEdge(ctx, a, unloaded) === null, '目标 Scene 未加载 → null（不写半条边）')
}

console.log('18. 端点路径描述与场景路径描述（诊断字段）')
{
  assert(describeScenePath([]) === 'root', '根 Scene 描述为 root')
  assert(describeScenePath(PATH_A) === 'root/embed:emb-a', '一层嵌入 → root/embed:<id>')
  assert(describeScenePath(PATH_A1) === 'root/embed:emb-a/embed:emb-a1', '两层嵌入逐段追加')
  assert(describeScenePath([{ type: 'enterPortal', instanceId: null, nodeId: 'p1' }]) === 'root/portal:p1',
    'Portal 段用 portal:<nodeId>')
  assert(describeTargetPath({ starmapId: 'sm-root', segments: [SEG_A], target: emptyTargetDetail('node', 'n1') })
    === 'root/embed:emb-a#node:n1', '端点路径描述带 scene 与 type:nodeId')
  assert(describeTargetPath({ starmapId: 'sm-root', segments: [SEG_A], target: emptyTargetDetail('starmap', null) })
    === 'root/embed:emb-a#starmap:', 'embed 端点描述为 starmap 且 nodeId 为空')
}

// ══════════════════════════════════════════════════════════════
// 10. 评审回归（#816 review）
// ══════════════════════════════════════════════════════════════

console.log('19. 评审回归 ①：捏合幅度来自两指距离，与 Scene 当前比例无关')
{
  const base = { x: 100, y: 100 }
  const baseDistance = fingerDistance(base, { x: 200, y: 100 })
  assert(near(baseDistance, 100), '起手两指距离就是两点间距')

  // 手指张开一倍 → 比例 2；合拢一半 → 0.5
  assert(near(computePinchRatio(baseDistance, [base, { x: 300, y: 100 }]), 2), '张开一倍 → 比例 2')
  assert(near(computePinchRatio(baseDistance, [base, { x: 150, y: 100 }]), 0.5), '合拢一半 → 比例 0.5')
  // 手指不动 → 比例 1，即使 Scene 自己的 scale 已经被前几帧改过
  assert(near(computePinchRatio(baseDistance, [base, { x: 200, y: 100 }]), 1),
    '手指没动 → 比例 1（老写法 owner.scale/起手 scale 会给出 ≠1，帧间自乘）')

  // 斜向张开也算真实间距：起手 100，现距 sqrt(200²+200²) = 200√2
  const diagonal = computePinchRatio(baseDistance, [base, { x: 300, y: 300 }])
  assert(near(diagonal, 2 * Math.SQRT2), '斜向张开按真实间距算比例，不是只比 x')

  // 起手距离为 0（只有一根手指）时退化成 1，不产生 NaN/Infinity
  assert(near(computePinchRatio(0, [base, { x: 200, y: 100 }]), 1), '起手距离为 0 → 比例 1')
  assert(near(computePinchRatio(baseDistance, [base]), 1), '不足两指 → 比例 1')
}

console.log('20. 评审回归 ②：注册表里的视口必须跟着 UI 主动 sync')
{
  const rects = [{ nodeId: 'n-root', x: 0, y: 0, width: 160, height: 80, radius: 16 }]
  const node = createSceneNode([], 'sm-root', { rects, scale: 1, offsetX: 0, offsetY: 0 })

  // 未 sync 之前：注册表是 scale=1 / offset=0，屏幕点 (50,40) 命中 n-root
  let ctx = buildRecursiveSceneContext(node)
  const before = resolveRecursiveHit(ctx, 50, 40)
  assert(before.target !== null && before.target.objectId === 'n-root', 'sync 前能命中根节点')

  // 现在画面上的真实视口已经变成 scale=2 / offset=(100,200)：
  // 屏幕点 (250,240) 对应画布 ((250-100)/2, (240-200)/2) = (75,20)，落在 n-root 内。
  // 但注册表还停在 1/0，同一个点会退化成画布 (250,240) → 空白。
  // 这就是 #816 评审第 2 条：缩放/平移改的是 @Link，注册表不会自己跟上。
  const stale = resolveRecursiveHit(ctx, 250, 240)
  assert(stale.target === null, '不 sync 的话，注册表按旧视口算，真实命中被判成空白')

  // 主动 sync 之后同一个点就命中了
  node.sync(rects, [], [], 2, 100, 200)
  ctx = buildRecursiveSceneContext(node)
  const fresh = resolveRecursiveHit(ctx, 250, 240)
  assert(fresh.target !== null && fresh.target.objectId === 'n-root',
    'sync 之后同一个屏幕点重新命中（syncSceneHandle 就是补这一下）')

  // 相机比例变了也要 sync：子 Scene 存的是父相机同步下来的那一份
  node.sync(rects, [], [], 0.5, 0, 0)
  ctx = buildRecursiveSceneContext(node)
  const refit = resolveRecursiveHit(ctx, 40, 20)
  assert(refit.target !== null && refit.target.objectId === 'n-root',
    '相机改完比例后 sync，句柄里的视口也跟着变（屏幕 (40,20) → 画布 (80,40)）')
  assert(node.zoomToScale === undefined && node.applyUserZoom === undefined && node.fitView === undefined,
    '句柄上没有任何缩放入口（#818）：缩放只走相机，不走 Scene 句柄')
}

console.log('21. 评审回归 ③：单指平移看 pan 归属，双指期间任何层都不能改视口（#818）')
{
  const tracker = createGestureStateTracker()
  // 普通单指拖动画布
  tracker.beginPanCanvas(PATH_A, 100, 100)
  assert(tracker.isPanOwnedByScene(PATH_A), '平移归属发起层')
  assert(tracker.isPinching() === false, '单指平移期间没有双指')
  // UI 的守卫条件：mode 是 panCanvas、panActive、pan 归属是自己、且本层可以改视口。
  // #818 之后 canMutateViewport 只剩 !isPinching()：相机是全局唯一的，
  // 双指期间任何一层平移都会和相机打架。
  const canPanCanvas = !tracker.isPinching()
  assert(canPanCanvas, '没有双指时本层可以平移')
  assert(tracker.isPanOwnedByScene([]) === false, '父层不归属这次平移')

  tracker.beginPinch(100, 100)
  const parentCanPan = !tracker.isPinching()
  assert(parentCanPan === false, '双指期间任何一层都自我否决平移，不会和相机同时改视口')
}

console.log('22. 评审回归 ④：连线结果必须回传，connect_end 才分得清失败原因')
{
  const plan = { hostStarmapId: 'sm-root' }
  const from = buildTargetPathForHost('sm-root', [], { scenePath: [], starmapId: 'sm-root', kind: 'node', itemId: 'n-root' })
  const to = buildTargetPathForHost('sm-root', [], { scenePath: PATH_A, starmapId: 'sm-a', kind: 'node', itemId: 'n-a1' })

  const okNode = createSceneNode([], 'sm-root', { createEdgeFn: async () => true })
  const badNode = createSceneNode([], 'sm-root', { createEdgeFn: async () => false })

  assert((await okNode.createEdgeBetween(from, to)) === true, 'Core 接受 → 回传 true，connect_end 记成功')
  assert((await badNode.createEdgeBetween(from, to)) === false,
    'Core 拒绝 → 回传 false，connect_end 记 core_failed（和"没找到目标"区分得开）')
  assert(plan.hostStarmapId === 'sm-root', '宿主 starmapId 一并带进诊断字段')
}

console.log('23. 评审回归 ⑤：捏合连着两帧，比例不能帧间自乘（#818 每帧从起手相机重算）')
{
  // 起手比例 1、两指距离 100。三帧：距离 110、120、130。
  const baseDistance = 100
  const pinch = createGlobalCameraPinch({ scale: 1, offsetX: 0, offsetY: 0 })
  pinch.begin([{ x: 0, y: 0 }, { x: baseDistance, y: 0 }])

  const applied = []
  for (const dist of [110, 120, 130]) {
    pinch.update([{ x: 0, y: 0 }, { x: dist, y: 0 }], dist / 2, 0)
    applied.push(pinch.camera.scale)
  }

  assert(near(applied[0], 1.1), '第一帧 100→110：1.00 × 1.1 = 1.10')
  assert(near(applied[1], 1.2), '第二帧 110→120：1.20，不是 1.10×1.2 = 1.32')
  assert(near(applied[2], 1.3), '第三帧 120→130：1.30，不是 1.716')
  assert(near(pinch.camera.scale, 1.3), '落在相机上的也是绝对值')

  // 反面对照：老写法"当前比例 × 本帧比例"就是这个发散结果
  const compounding = 1 * (110 / 100) * (120 / 100) * (130 / 100)
  assert(!near(applied[2], compounding, 1e-3), '收相对量会指数发散，绝对量不会')

  // #818：子 Scene 的 fitScale 不再参与这条链路，相机就是相机
  assert(near(applied[2], clampCameraScale(1 * 1.3)), '相机比例独立于任何子层 fitScale')
}

console.log('24. 评审回归 ⑥：相机缩放只走显示变换，子 Scene 的局部 fit 完全不动（#818/#821）')
{
  // 首次适配：直径 200 的圆，内容按内容包围盒算出 fitScale 并居中
  const rects = [
    { nodeId: 'a', x: 0, y: 0, width: 100, height: 100, radius: 0 },
    { nodeId: 'b', x: 100, y: 0, width: 100, height: 100, radius: 0 }
  ]
  const bounds = computeContentBounds(rects)
  // 可用区要扣掉父圆自己的标题/边框交互壳（#818 复审），所以这里和 fitView 用同一份口径
  const available = computeEmbedInnerContentSafeSide(200, EMBED_FIT_PADDING_VP)
  const fitted = computeFittedViewport(bounds, available, available, 0, 200, 200)

  // camera=1 时打开：局部 box 恒为 200，inheritedScale=1
  const view = createSceneView({ sceneDepth: 1, sceneWidth: 200, sceneHeight: 200, inheritedScale: 1 })
  view.fitView(rects)
  assert(near(view.fitScale, fitted.zoomScale) && near(view.fitOffsetX, fitted.offsetX),
    'camera=1 时算出的局部 fit 与直接用 200×200 算的一致')

  // 相机放到 2 倍 → 屏幕上圆壳和子 Scene 一起长大，inheritedScale 同步 1 → 2，
  // 但局部 box 尺寸恒为 DEFAULT_EMBED_DIAMETER，fit 一个字节都不改（#821）。
  view.cameraScale = 2
  view.inheritedScale = 2
  assert(view.sceneWidth === DEFAULT_EMBED_DIAMETER && view.sceneHeight === DEFAULT_EMBED_DIAMETER,
    '子 Scene 的 box 尺寸恒为本地 Embed 直径，不随相机变化')
  assert(near(view.fitScale, fitted.zoomScale) && near(view.fitOffsetX, fitted.offsetX) &&
    near(view.fitOffsetY, fitted.offsetY), 'fitScale / fitOffset 全部保持原值')
  assert(near(view.boxScale(), fitted.zoomScale),
    '显示变换用的是 boxScale（子层自己的 fitScale），不是累计比例')

  // 屏幕上最终呈现 = 局部 fit × 祖先累计 × 全局相机。
  // 相机围绕父画布中心放大 2 倍，子内容跟着圆壳一起长大，圆心仍钉在中心上。
  const camBefore = { scale: 1, offsetX: 0, offsetY: 0 }
  const camAfter = computeCameraZoomAround(camBefore, 200, 200, 2)
  const boundsCenterX = bounds.minX + bounds.width / 2
  const boundsCenterY = bounds.minY + bounds.height / 2
  const childToScreen = (cx, cy, sceneView, cam) => {
    const local = canvasToSceneLocal(
      cx, cy, sceneView.viewportScaleValue(), sceneView.viewportOffsetX(), sceneView.viewportOffsetY())
    return canvasToScreen(local.x, local.y, cam.scale, cam.offsetX, cam.offsetY)
  }
  const viewBefore = createSceneView({ sceneDepth: 1, sceneWidth: 200, sceneHeight: 200, inheritedScale: 1 })
  viewBefore.fitView(rects)
  const before = childToScreen(boundsCenterX, boundsCenterY, viewBefore, camBefore)
  const after = childToScreen(boundsCenterX, boundsCenterY, view, camAfter)
  assert(near(before.x, 100) && near(before.y, 100), '缩放前内容中心在 200×200 圆壳的中心 (100,100)')
  assert(near(after.x, 200) && near(after.y, 200),
    '相机 ×2 后内容中心跟着圆壳走到 (200,200)（局部 fit 不变，屏幕上同比放大）')
  assert(near(view.viewportScaleValue(), 2 * fitted.zoomScale),
    '子 Scene 的有效比例 = fitScale × inheritedScale，随相机一起长大')

  // 缩小同样成立
  view.cameraScale = 0.5
  view.inheritedScale = 0.5
  assert(near(view.fitScale, fitted.zoomScale),
    '缩回一半：局部 box 还是 200，fitScale 依旧不动')
}

console.log('24b. 变换链：camera 1 → 2 时根 / 子 / 孙三层内容全部 ×2（#818 复审）')
{
  const childRects = [{ nodeId: 'n2', x: 0, y: 0, width: 56, height: 56, radius: 28 }]
  const grandRects = [{ nodeId: 'n3', x: 0, y: 0, width: 56, height: 56, radius: 0 }]

  // 建三层：根（相机本体）+ 子 + 孙。每层 box 尺寸恒为本地 DEFAULT_EMBED_DIAMETER（#821），
  // 屏幕上的放大全部由累计比例体现。
  const build = (cameraScale) => {
    const root = createSceneView({ sceneDepth: 0, cameraScale })
    const child = createSceneView({
      sceneDepth: 1, inheritedScale: root.viewportScaleValue(),
      sceneWidth: DEFAULT_EMBED_DIAMETER, sceneHeight: DEFAULT_EMBED_DIAMETER
    })
    child.fitView(childRects)
    const grand = createSceneView({
      sceneDepth: 2, inheritedScale: child.viewportScaleValue(),
      sceneWidth: DEFAULT_EMBED_DIAMETER, sceneHeight: DEFAULT_EMBED_DIAMETER
    })
    grand.fitView(grandRects)
    return { root, child, grand }
  }

  const a = build(1)
  const b = build(2)

  assert(near(b.root.viewportScaleValue() / a.root.viewportScaleValue(), 2), '根层比例 1 → 2')
  assert(near(b.child.viewportScaleValue() / a.child.viewportScaleValue(), 2), '子层节点屏幕尺寸 ×2')
  assert(near(b.grand.viewportScaleValue() / a.grand.viewportScaleValue(), 2), '二层子星图内节点屏幕尺寸 ×2')
  assert(near(a.child.fitScale, b.child.fitScale) && near(a.grand.fitScale, b.grand.fitScale),
    '各层局部 fit 本身保持不变（连乘关系，camera 不会重复进每层）')
  assert(near(b.grand.viewportScaleValue(), 2 * b.child.fitScale * b.grand.fitScale),
    '孙层有效比例 = camera × fit₁ × fit₂（camera 没有被重复乘两次）')
  assert(near(b.child.sceneWidth, DEFAULT_EMBED_DIAMETER) &&
    near(b.grand.sceneWidth, DEFAULT_EMBED_DIAMETER),
    '每层局部 box 恒等于 Embed 基础直径，不随相机变化')

  // 写进注册表的快照必须是累计值，递归命中那套换算才对得上画面
  const snap = b.child.sceneSourceSnapshot()
  assert(near(snap.scale, b.child.viewportScaleValue()) && near(snap.offsetX, b.child.viewportOffsetX()),
    'sync() 写入的是累计有效比例 / 累计偏移')
}

console.log('24c. 子图在相机 1 和相机 2 下打开，局部 fit 必须一模一样（#818 复审）')
{
  const rects = [
    { nodeId: 'a', x: 0, y: 0, width: 100, height: 100, radius: 0 },
    { nodeId: 'b', x: 100, y: 0, width: 100, height: 100, radius: 0 }
  ]
  const openAt = (cameraScale) => {
    // 根层相机 = cameraScale → inheritedScale 同值；局部 box 恒为 200
    const root = createSceneView({ sceneDepth: 0, cameraScale })
    const child = createSceneView({
      sceneDepth: 1, inheritedScale: root.viewportScaleValue(),
      sceneWidth: DEFAULT_EMBED_DIAMETER, sceneHeight: DEFAULT_EMBED_DIAMETER
    })
    child.fitView(rects)
    return child
  }
  const at1 = openAt(1)
  const at2 = openAt(2)
  assert(near(at1.fitScale, at2.fitScale) && near(at1.fitOffsetX, at2.fitOffsetX) &&
    near(at1.fitOffsetY, at2.fitOffsetY),
  '同样的内容 + 同样的局部尺寸 → 同样的局部 fit（与打开时的相机无关）')
  assert(near(at2.viewportScaleValue() / at1.viewportScaleValue(), 2),
    '但屏幕上的呈现仍然是相机 ×2')

  // #821：局部 box 不再会因为"父 Embed 基础尺寸变了"而重算 fit —— 那个尺寸恒为 200。
  // 换视口 / 换相机只改显示变换，fit 一次算完就不动。
  const fitForDiameter = (contentRects) => {
    const bounds = computeContentBounds(contentRects)
    const available = computeEmbedInnerContentSafeSide(DEFAULT_EMBED_DIAMETER, EMBED_FIT_PADDING_VP)
    return computeFittedViewport(bounds, available, available, 0,
      DEFAULT_EMBED_DIAMETER, DEFAULT_EMBED_DIAMETER).zoomScale
  }
  assert(near(at1.fitScale, fitForDiameter(rects)),
    '局部 fit 恒按 DEFAULT_EMBED_DIAMETER 的可用区算，不按任何"展开尺寸"')
  assert(near(at2.fitScale, fitForDiameter(rects)),
    '相机放大不改变 fit 的口径（同上）')
}

console.log('24d. 任意一层空白拖动都只改全局相机：子 Scene 不再有独立平移相机（#818 复审）')
{
  // 子星图不是独立窗口。手指在子图空白里拖，目标 offset 原样回传给根，
  // 子 Scene 的 fitOffsetX/Y 一个像素都不动。
  const view = createSceneView({
    sceneDepth: 1, inheritedScale: 2,
    sceneWidth: DEFAULT_EMBED_DIAMETER, sceneHeight: DEFAULT_EMBED_DIAMETER
  })
  view.fitView([{ nodeId: 'n', x: 0, y: 0, width: 100, height: 100, radius: 0 }])
  const fitOffsetBefore = { x: view.fitOffsetX, y: view.fitOffsetY }
  let forwarded = null
  view.applyCameraPanTo(20, -10, (offsetX, offsetY) => { forwarded = { offsetX, offsetY } })
  assert(forwarded !== null && forwarded.offsetX === 20 && forwarded.offsetY === -10,
    '子 Scene 不改自己的偏移，把目标 offset 交给根（onCameraPanTo）')
  assert(view.fitOffsetX === fitOffsetBefore.x && view.fitOffsetY === fitOffsetBefore.y,
    '子 Scene 的 fitOffsetX/Y 在拖动画布时完全不变（它只由 fitView 写）')
  assert(near(view.viewportOffsetX(), fitOffsetBefore.x * 2),
    '屏幕偏移仍等于局部 fitOffset × 祖先累计比例')

  // 根 Scene 自己拖：直接整份改全局相机，不做换算
  const root = createSceneView({ sceneDepth: 0 })
  root.applyCameraPanTo(20, -10, () => { throw new Error('根 Scene 不该回传 onCameraPanTo') })
  assert(near(root.cameraOffsetX, 20) && near(root.cameraOffsetY, -10), '根 Scene 改的是全局相机，不做换算')

  // 祖先比例为 0 时的兜底：不能把整棵子树的尺寸算成 0
  const broken = createSceneView({ sceneDepth: 1, inheritedScale: 0 })
  assert(near(broken.parentScale(), 1) && near(broken.viewportScaleValue(), broken.fitScale),
    'inheritedScale 传成 0 时兜回 1，不让子树塌成 0 倍')
}

console.log('24e. 子星图里的节点 / 子星图不能被拖到圆外：写入前夹进圆内接正方形（#818 复审）')
{
  // 直径 200 的圆：内接正方形 200/√2 ≈ 141.4，扣掉父圆自己的标题+边框交互壳 36，
  // 再扣掉两边留白 16 → 安全区 ≈ 89.4
  const safeSize = computeEmbedInnerContentSafeSide(200, EMBED_FIT_PADDING_VP)
  const center = 100
  const minEdge = center - safeSize / 2
  const maxEdge = center + safeSize / 2
  // 用能整个塞进安全区的 item（40×40），边界好手算核对
  const item = 40

  // fitScale = 1 / 偏移 0 时最容易直接手算核对
  const draggedFar = clampItemToEmbedSafeArea(9999, -9999, item, item, 1, 0, 0, 200, EMBED_FIT_PADDING_VP)
  assert(near(draggedFar.x, maxEdge - item) && near(draggedFar.y, minEdge),
    '往右下拖到底：item 完整贴住安全区右下角')
  assert(near(draggedFar.x, minEdge) || draggedFar.x > minEdge, '横向被夹在安全区内')
  assert(draggedFar.x + item <= maxEdge + 1e-9 && draggedFar.y >= minEdge - 1e-9,
    '显示矩形完整落在安全区里，一像素都不出圆')

  // 在安全区内的正常坐标不该被改动
  const inside = clampItemToEmbedSafeArea(80, 85, item, item, 1, 0, 0, 200, EMBED_FIT_PADDING_VP)
  assert(near(inside.x, 80) && near(inside.y, 85), '本来就在安全区内的坐标原样返回')

  // item 比整个安全区还大时那一轴没有合法区间，退回居中（默认 160 宽的节点就属于这种：
  // 200 直径的圆扣掉留白后安全区只有 ~125，宽 160 放不下）
  const tooWide = clampItemToEmbedSafeArea(9999, 9999, 160, 80, 1, 0, 0, 200, EMBED_FIT_PADDING_VP)
  assert(near(tooWide.x + 80, center), '宽度超过安全区时横向退回居中，不会贴着一侧溢出')
  assert(near(tooWide.y + 80, maxEdge), '纵向仍有合法区间，照常夹到安全区下沿')

  // 圆形 Embed 同样不出圆（按直径算完整显示矩形）
  const embed = clampItemToEmbedSafeArea(5000, 5000, item, item, 1, 0, 0, 200, EMBED_FIT_PADDING_VP)
  assert(near(embed.x, maxEdge - item) && near(embed.y, maxEdge - item),
    '子星图本身也不能被拖到只剩半个圆壳在区内')

  // fitScale / fitOffset 不为 1/0 时，换算往返仍然闭合
  const withFit = clampItemToEmbedSafeArea(300, 300, item, item, 0.5, 20, 30, 200, EMBED_FIT_PADDING_VP)
  const displayLeft = withFit.x * 0.5 + 20
  const displayTop = withFit.y * 0.5 + 30
  assert(displayLeft >= minEdge - 1e-9 && displayLeft + item * 0.5 <= maxEdge + 1e-9,
    '带局部适配时，夹的是换算到局部显示坐标之后的矩形')
  assert(displayTop >= minEdge - 1e-9 && displayTop + item * 0.5 <= maxEdge + 1e-9,
    '纵向同理')

  // 容器还没量出来时原样返回，不在这里把坐标改成 NaN
  const noSize = clampItemToEmbedSafeArea(123, 456, item, item, 1, 0, 0, 0, EMBED_FIT_PADDING_VP)
  assert(noSize.x === 123 && noSize.y === 456, 'localSceneSize 还是 0 时原样返回，不改数据')
  const nanIn = clampItemToEmbedSafeArea(NaN, 0, item, item, 1, 0, 0, 200, EMBED_FIT_PADDING_VP)
  assert(Number.isNaN(nanIn.x), '输入坐标坏掉时原样返回')

  // Scene 视角：根层自由移动，子层走安全区
  const rootView = createSceneView({ sceneDepth: 0, sceneWidth: 400, sceneHeight: 400 })
  const freeMove = rootView.clampItemToLocalSafeArea(5000, -5000, item, item)
  assert(freeMove.x === 5000 && freeMove.y === -5000, '根 Scene 的对象仍然自由移动（整屏画布没有圆壳）')

  const childView = createSceneView({ sceneDepth: 1, inheritedScale: 1, sceneWidth: 200, sceneHeight: 200 })
  childView.fitView([{ nodeId: 'n', x: 0, y: 0, width: 100, height: 100, radius: 0 }])
  const clampedMove = childView.clampItemToLocalSafeArea(5000, -5000, item, item)
  assert(clampedMove.x !== 5000 && clampedMove.y !== -5000,
    '子 Scene 的同一份拖动被夹回圆内：显示矩形跟着本层 fitScale / fitOffset 走')

  // 祖先累计比例变了（相机缩放）也不影响夹出来的结果，因为全程用局部数据
  const childAt2x = createSceneView({ sceneDepth: 1, inheritedScale: 2, sceneWidth: 400, sceneHeight: 400 })
  childAt2x.fitView([{ nodeId: 'n', x: 0, y: 0, width: 100, height: 100, radius: 0 }])
  const clampedAt2x = childAt2x.clampItemToLocalSafeArea(5000, -5000, item, item)
  assert(near(clampedAt2x.x, clampedMove.x) && near(clampedAt2x.y, clampedMove.y),
    '相机放大两倍后夹出来的局部坐标一模一样（安全区是局部数据，与相机无关）')
}

console.log('24f. 双指 > 所有单指：pinch 认领后单指 begin* 一律改不动 mode（#818 复审）')
{
  const t = createGestureStateTracker()
  t.beginPinch(460, 60)
  t.beginPanCanvas(PATH_A, 0, 0)
  assert(t.getState().mode === 'pinch', 'pinch 进行中，晚到的 beginPanCanvas 不能改写 mode')
  t.beginNodeMenu(PATH_A, 'n1', 0, 0)
  assert(t.getState().mode === 'pinch', 'beginNodeMenu 同样改不动')
  t.beginConnect(PATH_A, 'n1', 0, 0)
  assert(t.getState().mode === 'pinch', 'beginConnect 同样改不动')
  t.beginMoveNode(PATH_A, 'n1', 0, 0)
  assert(t.getState().mode === 'pinch', 'beginMoveNode 同样改不动')
  t.beginMoveEmbed(PATH_A, 'e1', 0, 0)
  assert(t.getState().mode === 'pinch', 'beginMoveEmbed 同样改不动')
  assert(t.getState().activeItemId === '', 'activeItemId 也没被单指写进来')
  assert(t.getState().panOwnerScenePath === null && t.getState().connectOwnerScenePath === null,
    'pan / connect 归属都没被单指占用')

  // pinch 结束之后单指才重新能写状态
  t.endPinch()
  t.beginPanCanvas(PATH_A, 0, 0)
  assert(t.getState().mode === 'panCanvas', '双指抬起后单指恢复写状态')
}

console.log('24g. 拖画布必须按"起手基准 + 累计位移"算目标，不能把每帧当成增量（#818 复审）')
{
  // ArkUI 的 PanGesture event.offsetX/Y 是相对手势起点的累计位移。
  // 真实手指只拖了 15vp；如果 camera += event.offsetX，
  // 5 / 10 / 15 三帧会被叠成 30vp，拖得越久越离谱。
  const root = createSceneView({ sceneDepth: 0, cameraOffsetX: 100, cameraOffsetY: 40 })
  // 根 Scene 自己拖：起手记基准，每帧算 基准 + 累计位移
  const panRoot = (baseX, baseY, frames) => {
    root.cameraOffsetX = baseX
    root.cameraOffsetY = baseY
    for (const frame of frames) {
      root.applyCameraPanTo(panBaseCameraOffsetX + frame.dx, panBaseCameraOffsetY + frame.dy, () => {
        throw new Error('根 Scene 不该回传')
      })
    }
  }
  let panBaseCameraOffsetX = 100
  let panBaseCameraOffsetY = 40
  panRoot(100, 40, [{ dx: 5, dy: 5 }, { dx: 10, dy: 10 }, { dx: 15, dy: 15 }])
  assert(near(root.cameraOffsetX, 115) && near(root.cameraOffsetY, 55),
    '根 Scene：base 100 + 累计 15 = 115（不是 5+10+15=130）')

  // 子 Scene 拖空白：目标 offset 同样按 基准 + 累计位移 算好再回传给根
  const child = createSceneView({
    sceneDepth: 1, inheritedScale: 2, sceneWidth: 400, sceneHeight: 400,
    cameraOffsetX: 100, cameraOffsetY: 40
  })
  child.fitView([{ nodeId: 'n', x: 0, y: 0, width: 100, height: 100, radius: 0 }])
  const childFitBefore = { x: child.fitOffsetX, y: child.fitOffsetY }
  const rootCamera = createSceneView({ sceneDepth: 0, cameraOffsetX: 100, cameraOffsetY: 40 })
  const onCameraPanTo = (offsetX, offsetY) => {
    rootCamera.applyCameraPanTo(offsetX, offsetY, () => { throw new Error('根 Scene 不该回传') })
  }
  panBaseCameraOffsetX = child.cameraOffsetX
  panBaseCameraOffsetY = child.cameraOffsetY
  for (const frame of [{ dx: 5, dy: 5 }, { dx: 10, dy: 10 }, { dx: 15, dy: 15 }]) {
    child.applyCameraPanTo(panBaseCameraOffsetX + frame.dx, panBaseCameraOffsetY + frame.dy, onCameraPanTo)
  }
  assert(near(rootCamera.cameraOffsetX, 115) && near(rootCamera.cameraOffsetY, 55),
    '子 Scene：累计位移一样只算一次，回传给根的目标也是 115')
  assert(near(child.fitOffsetX, childFitBefore.x) && near(child.fitOffsetY, childFitBefore.y),
    '子 Scene 的局部 fit 偏移在整个拖动过程里一动不动')

  // 第二次拖动必须以上一次结束的位置为新基准，而不是接着上次继续累加
  panBaseCameraOffsetX = rootCamera.cameraOffsetX
  panBaseCameraOffsetY = rootCamera.cameraOffsetY
  for (const frame of [{ dx: 5, dy: 0 }, { dx: 8, dy: 0 }]) {
    rootCamera.applyCameraPanTo(panBaseCameraOffsetX + frame.dx, panBaseCameraOffsetY + frame.dy, () => {})
  }
  assert(near(rootCamera.cameraOffsetX, 123), '第二次拖动从 115 出发，8vp 后是 123')
}

console.log('24h. 空子星图第一次新建也要守边界：先 clamp 再整体重新 local fit（#818 复审）')
{
  // 场景：空 child 首次 fitView() 时没有内容 → fitScale = 1 且 hasFittedView = true，
  // 之后 maybeFitView 不再跑。少了重新 fit，新建的 160×80 节点会直接大于当前安全区。
  const safeSize = computeEmbedInnerContentSafeSide(200, EMBED_FIT_PADDING_VP)
  const center = 100
  const minEdge = center - safeSize / 2
  const maxEdge = center + safeSize / 2

  const view = createSceneView({ sceneDepth: 1, inheritedScale: 1, sceneWidth: 200, sceneHeight: 200 })
  view.fitView([])
  assert(near(view.fitScale, 1) && view.hasFittedView === true,
    '空子星图首次 fit：没有内容 → fitScale 停在 1，且已经算过一次')

  // 在圆边缘长按新建：160×80 的节点比当前安全区（≈89.4）还宽，X 方向 clamp 只能把它居中；
  // Y 方向还放得下（80 < 89.4），照常夹到下边界。真正让它完整进圆的是后面的重新 local fit。
  const nodeCandidate = { x: 196, y: 196 }
  const clampedNode = view.clampItemToLocalSafeArea(
    nodeCandidate.x, nodeCandidate.y, DEFAULT_NODE_WIDTH, DEFAULT_NODE_HEIGHT)
  assert(near(clampedNode.x + DEFAULT_NODE_WIDTH / 2, center),
  '比安全区还宽的新建节点在 X 方向被居中（放不下时夹不到边界，只能居中）')
  assert(near(clampedNode.y + DEFAULT_NODE_HEIGHT, maxEdge),
  '放得下的 Y 方向仍被夹到安全区下边界，不再往圆外落')

  // 新建成功后重新 local fit：160×80 的内容会缩到刚好塞进安全区
  const nodeRects = [{ nodeId: 'n1', x: clampedNode.x, y: clampedNode.y, width: DEFAULT_NODE_WIDTH, height: DEFAULT_NODE_HEIGHT, radius: 0 }]
  const fitBeforeCreate = view.fitScale
  assert(view.refitAfterContentAdded(nodeRects) === true && !near(view.fitScale, fitBeforeCreate),
    '新建节点后重新 local fit，fitScale 从 1 缩小')
  const displayWidth = DEFAULT_NODE_WIDTH * view.fitScale
  assert(displayWidth <= safeSize, '缩放后节点完整宽度小于等于安全区边长')
  const nodeLeft = clampedNode.x * view.fitScale + view.fitOffsetX
  const nodeRight = nodeLeft + displayWidth
  assert(nodeLeft >= minEdge - 0.5 && nodeRight <= maxEdge + 0.5,
    '重新 fit 后节点完整显示矩形落在安全区内，不会被父圆裁掉')

  // 空子星图新建二级子星图：200 的圆不能顶满父圆
  const embedChild = createSceneView({ sceneDepth: 1, inheritedScale: 1, sceneWidth: 200, sceneHeight: 200 })
  embedChild.fitView([])
  const embedCandidate = { x: 195, y: 195 }
  const clampedEmbed = embedChild.clampItemToLocalSafeArea(
    embedCandidate.x, embedCandidate.y, DEFAULT_EMBED_DIAMETER, DEFAULT_EMBED_DIAMETER)
  const embedRects = [{
    nodeId: 'e1', x: clampedEmbed.x, y: clampedEmbed.y,
    width: DEFAULT_EMBED_DIAMETER, height: DEFAULT_EMBED_DIAMETER, radius: DEFAULT_EMBED_DIAMETER / 2
  }]
  assert(embedChild.refitAfterContentAdded(embedRects) === true && embedChild.fitScale < 1,
    '新建二级子星图后 local fit 自动缩小')
  assert(DEFAULT_EMBED_DIAMETER * embedChild.fitScale < DEFAULT_EMBED_DIAMETER,
    '二级子星图的显示尺寸严格小于父 Embed')
  assert(DEFAULT_EMBED_DIAMETER * embedChild.fitScale <= safeSize + 0.5,
    '二级子星图完整落在父圆内接正方形里，不被 clip 裁掉')

  // 全局相机不能被重新 local fit 影响
  const camBefore = { scale: view.cameraScale, offsetX: view.cameraOffsetX, offsetY: view.cameraOffsetY }
  view.refitAfterContentAdded(nodeRects)
  assert(view.cameraScale === camBefore.scale && view.cameraOffsetX === camBefore.offsetX &&
    view.cameraOffsetY === camBefore.offsetY,
  '重新 local fit 不碰全局相机，用户双指缩放好的视角不会被重置')

  // 根 Scene 不做 local fit：整屏显示，没有圆壳
  const root = createSceneView({ sceneDepth: 0 })
  assert(root.refitAfterContentAdded(nodeRects) === false, '根 Scene 不需要重新 local fit')
  assert(eq(root.clampItemToLocalSafeArea(9999, -9999, DEFAULT_NODE_WIDTH, DEFAULT_NODE_HEIGHT), { x: 9999, y: -9999 }),
    '根 Scene 的新建位置不夹，自由落点')
}

console.log('')
console.log('24i. 契约 5e：相机状态链不能断在中间层，子 Scene 起手 Pan 必须读到当前全局 offset（#818 复审）')

// 真机时序：根层先拖到 cameraOffsetX = 100，此时 EmbedScene 的普通字段还停在初始化时的 0；
// 再在子星图里起手 Pan，panBaseCameraOffsetX 拿到旧值 0，第一帧 +5 就把全局相机写成 5。
// 模拟这条链：@Prop 是持续同步，普通字段只做一次初始化。
{
  const makePropChain = () => {
    const screen = { cameraScale: 1, cameraOffsetX: 0, cameraOffsetY: 0 }
    const root = createSceneView({ sceneDepth: 0 })
    const embedScene = { cameraScale: 1, cameraOffsetX: 0, cameraOffsetY: 0 }
    const child = createSceneView({ sceneDepth: 1, inheritedScale: 1, sceneWidth: 200, sceneHeight: 200 })
    child.fitView([{ nodeId: 'n', x: 0, y: 0, width: 100, height: 100, radius: 0 }])
    const onCameraPanTo = (offsetX, offsetY) => {
      screen.cameraOffsetX = offsetX
      screen.cameraOffsetY = offsetY
      syncFromScreen()
    }
    // @Prop 语义：父值一变，子值跟着变。真实 ArkUI 里这是框架保证的同步，
    // 这里把它显式建模出来，才能看出中间层写成普通字段会断在哪。
    const syncFromScreen = () => {
      root.cameraScale = screen.cameraScale
      root.cameraOffsetX = screen.cameraOffsetX
      root.cameraOffsetY = screen.cameraOffsetY
      embedScene.cameraScale = root.cameraScale
      embedScene.cameraOffsetX = root.cameraOffsetX
      embedScene.cameraOffsetY = root.cameraOffsetY
      child.cameraScale = embedScene.cameraScale
      child.cameraOffsetX = embedScene.cameraOffsetX
      child.cameraOffsetY = embedScene.cameraOffsetY
    }
    // 模拟"普通字段只拿父值做一次初始化，之后不再同步"的那一层
    const syncFromScreenBroken = () => {
      root.cameraScale = screen.cameraScale
      root.cameraOffsetX = screen.cameraOffsetX
      root.cameraOffsetY = screen.cameraOffsetY
      // embedScene 这一层只有初始化，之后父值改了不同步
      child.cameraScale = embedScene.cameraScale
      child.cameraOffsetX = embedScene.cameraOffsetX
      child.cameraOffsetY = embedScene.cameraOffsetY
    }
    return { screen, root, embedScene, child, onCameraPanTo, syncFromScreen, syncFromScreenBroken }
  }

  const panFromChild = (chain, sync) => {
    let baseX = chain.child.cameraOffsetX
    let baseY = chain.child.cameraOffsetY
    for (const frame of [{ dx: 5, dy: 0 }]) {
      chain.child.applyCameraPanTo(baseX + frame.dx, baseY + frame.dy, chain.onCameraPanTo)
      sync()
    }
  }

  // 正常链：根层先拖到 100，再从子星图起手
  const ok = makePropChain()
  ok.screen.cameraOffsetX = 100
  ok.screen.cameraOffsetY = 60
  ok.syncFromScreen()
  assert(ok.child.cameraOffsetX === 100 && ok.child.cameraOffsetY === 60,
    '中间层是 @Prop：根层拖到 100/60 之后，child 读到的是 100/60')
  panFromChild(ok, ok.syncFromScreen)
  assert(ok.screen.cameraOffsetX === 105 && ok.screen.cameraOffsetY === 60,
    '从子星图起手 Pan +5：全局相机是 105/60，绝不跳回 5')

  // 断链对照：中间层写成普通字段，child 停在初始化时的旧值
  const broken = makePropChain()
  broken.screen.cameraOffsetX = 100
  broken.syncFromScreenBroken()
  assert(broken.child.cameraOffsetX === 0, '对照：中间层是普通字段 → child 还停在 0（这就是要防的 bug）')
  panFromChild(broken, broken.syncFromScreenBroken)
  assert(broken.screen.cameraOffsetX === 5,
    '对照：旧基准 0 + 5 = 5，全局相机从 100 跳回 5（复现真机症状）')

  // 结构守卫：真实 .ets 里这三个字段必须还是 @Prop，不能退回普通字段
  const embedSource = readStarmapSource('ui/StarMapEmbedScene.ets')
  for (const field of ['cameraScale', 'cameraOffsetX', 'cameraOffsetY']) {
    assert(new RegExp(`@Prop\\s+${field}\\s*:`).test(embedSource),
      `StarMapEmbedScene.ets 里 ${field} 仍是 @Prop（ArkUI V1 只有 @Prop 才是父→子单向同步）`)
    assert(!new RegExp(`^\\s{2}${field}\\s*:`, 'm').test(embedSource),
      `${field} 没有退回成不带装饰器的普通字段`)
  }
  assert(embedSource.includes('cameraScale: this.cameraScale'),
    'EmbedScene 仍然把 cameraScale 透传给 child StarMapScene')
  assert(/onCameraPanTo[\s\S]*offsetX: number, offsetY: number/.test(embedSource),
    'onCameraPanTo 保持普通字段即可：它是稳定引用，不是每帧变化的状态')
}

console.log('')
console.log('25. 时序回归 ⑦：旧单指手势的收尾不能把刚认领的 Pinch 一起清掉')

// 这正是用户最早报的"只要有一根手指按在子星图上，双指就容易缩放不了"：
// 第一根手指先让子层 Pan 认领，第二根手指落下后根 Scene 把全局切成 pinch，
// 旧 Pan 的 onActionEnd / onActionCancel 如果无条件 reset，就把新 Pinch 一起清了。

// 25.1 老写法必然误伤：beginPinch 把 ownerScenePath 也写成归属层，
//      于是同一层的旧 Pan 用 isOwnedByScene 判定仍然是 true。
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  t.beginPinch(0, 0)
  assert(t.isOwnedByScene(PATH_A) === false,
    '新判定：beginPinch 不再把 ownerScenePath 写成任何子层，旧 Pan 的 isOwnedByScene 已经为 false')
  // 老代码在这里无条件 reset() → pinch 被清掉
  t.reset()
  assert(!t.isPinching(), '老写法 reset() 之后 pinch 没了（这就是 #816 的症状来源）')
}

// 25.2 新写法：mode-aware 清空，旧 Pan 收尾时 mode 已经不是 panCanvas，拒绝清
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  t.beginPinch(0, 0)
  const cleared = t.clearIfOwnedBy('panCanvas', PATH_A)
  assert(!cleared, 'mode 已是 pinch → clearIfOwnedBy(panCanvas) 拒绝清空')
  assert(t.isPinching(), '旧 Pan 的收尾之后 pinch 仍然活着')
  assert(t.getState().mode === 'pinch', 'mode 仍是 pinch')
  assert(eq(t.getState().ownerScenePath, []), 'pinch 归属根（相机在根）')
}

// 25.3 父层的旧 Pan 同样要验：mode 不匹配就不能清别人的手势
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  t.beginPinch(0, 0)
  assert(!t.clearIfOwnedBy('panCanvas', PATH_A), '双指进行中，旧 Pan 收尾不能清 pinch')
  assert(t.isPinching(), 'pinch 没被破坏')
}

// 25.4 没有被接管时，正常收尾仍然要清干净（不能修过头）
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  assert(t.clearIfOwnedBy('panCanvas', PATH_A), '没人接管时旧 Pan 收尾照常清空')
  assert(t.isIdle(), '清空后回到 idle')
}

// 25.5 旧 connect 的收尾同样不能误伤 pinch
{
  const t = createGestureStateTracker()
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  t.beginPinch(0, 0)
  assert(!t.clearIfOwnedBy('connect', PATH_A), 'mode 已是 pinch → 旧 connect 收尾只能清预览')
  assert(t.isPinching(), '旧 connect 的 onActionEnd 之后 pinch 仍然活着')
}

// 25.6 旧 connect 正常收尾仍然要清
{
  const t = createGestureStateTracker()
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  assert(t.clearIfOwnedBy('connect', PATH_A), '没人接管时旧 connect 收尾照常清空')
  assert(t.isIdle(), '清空后回到 idle')
}

// 25.7 菜单消失回调同理：本地标记可能是旧值，必须靠全局 mode 把关
{
  const t = createGestureStateTracker()
  t.beginNodeMenu(PATH_A, 'n-a1', 0, 0)
  t.beginPinch(0, 0)
  assert(!t.clearIfOwnedBy('nodeMenu', PATH_A), 'mode 已是 pinch → 菜单消失回调不能清 pinch')
  assert(t.isPinching(), 'onNodeMenuDisappear 之后 pinch 仍然活着')
}

// 25.8 moveNode 被取消时不落盘，但动画必须恢复；且不能误伤 pinch
{
  const t = createGestureStateTracker()
  t.beginMoveNode(PATH_A, 'n-a1', 0, 0)
  t.beginPinch(0, 0)
  const state = t.getState()
  const owns = state.mode === 'moveNode' && state.activeItemId === 'n-a1' &&
    t.isActiveItemInScene(PATH_A)
  assert(!owns, '被 pinch 接管后 moveNode 不再归自己 → 不落盘')
  assert(t.isPinching(), '取消移动不影响 pinch')
}

console.log('')
console.log('26. 取消路径回归 ⑧：Cancel 不能把 tracker 卡在旧 mode，也不能留下本地残值')

// 26.1 connect 被取消且仍是自己 → 必须清回 idle，
//     否则 tracker 卡在 connect，后续别层手势会被"不是 idle、owner 也不是我"全部挡住
{
  const t = createGestureStateTracker()
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  const cleared = t.clearIfOwnedBy('connect', PATH_A)
  assert(cleared, '仍是自己的 connect → 取消收尾清回 idle')
  assert(t.isIdle(), '取消后 tracker 回到 idle，不会卡在 connect')
}

// 26.2 connect 被取消但已被 pinch 接管 → 只清预览，pinch 必须活着
{
  const t = createGestureStateTracker()
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  t.beginPinch(0, 0)
  const cleared = t.clearIfOwnedBy('connect', PATH_A)
  assert(!cleared, '已被 pinch 接管 → 取消收尾拒绝清全局')
  assert(t.isPinching(), '取消旧 connect 之后 pinch 仍然活着')
  assert(t.getState().mode === 'pinch', 'mode 仍是 pinch')
}

// 26.3 取消后别层手势不会被挡住：idle 时任何一层都能起手
{
  const t = createGestureStateTracker()
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  t.clearIfOwnedBy('connect', PATH_A)
  assert(t.isIdle(), '取消后是 idle')
  t.beginPanCanvas(PATH_B, 0, 0)
  assert(t.isPanOwnedByScene(PATH_B), '取消后兄弟 Scene 能正常起手（没被卡住）')
}

// 26.4 本地 gestureMode 必须无条件收回：被 pinch 接管时也一样
//     老写法 `if (cleared) { gestureMode = 'idle' }` 会让本地停在 panCanvas，
//     造成 global=pinch / local=panCanvas 的分叉。
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  t.beginPinch(0, 0)
  const cleared = t.clearIfOwnedBy('panCanvas', PATH_A)
  // 新写法：不管清没清掉，本地旧模式都结束
  assert(!cleared, '全局没被清（mode 已经切成 pinch）')
  assert(t.getState().mode === 'pinch', '全局仍是 pinch')
  assert(t.isPinching(), 'pinch 存活；本地旧 pan 模式此时已经收尾，不会再影响新手势')
}

// 26.5 没人接管时旧 move 收尾：落盘 + 清全局 + 收回本地
{
  const t = createGestureStateTracker()
  t.beginMoveNode(PATH_A, 'n-a1', 0, 0)
  const state = t.getState()
  const owns = state.mode === 'moveNode' && state.activeItemId === 'n-a1' &&
    t.isActiveItemInScene(PATH_A)
  assert(owns, '没人接管时 moveNode 仍归自己 → 落盘')
  assert(t.clearIfOwnedBy('moveNode', PATH_A), '落盘后清全局')
  assert(t.isIdle(), '回到 idle')
}

console.log('')
console.log('27. 生命周期回归 ⑨：无关 Scene 退场不能清掉全树唯一的 tracker')

// 树里所有 Scene 共用同一个 tracker。子 Scene 因为 Embed 被删、childGraph 重载、
// loading/error 分支切换而单独 aboutToDisappear 时，不能把别人的手势清掉。
const PATH_ROOT = []
const PATH_C = [embedSegment('emb-c')]

// 27.1 owner 在 emb-a，emb-c 的 Scene 退场 → tracker 不变
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  const shouldReset = PATH_C.length === 0 || t.isOwnedBySceneSubtree(PATH_C)
  assert(!shouldReset, 'owner 在 emb-a 时，emb-c 的 Scene 退场不清 tracker')
  assert(!t.isIdle(), '兄弟子树的 pan 仍然活着')
  assert(t.isPanOwnedByScene(PATH_A), '归属层没被误清')
}

// 27.2 owner 在 emb-a/emb-a1（孙层），emb-a 的 Scene 退场 → 必须清
//     这条锁的是"子树前缀"而不是"完全相等"：owner 不是 emb-a 本身
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A1, 0, 0)
  assert(!t.isOwnedByScene(PATH_A), '孙层的 owner 不等于父 Scene 本身')
  assert(t.isOwnedBySceneSubtree(PATH_A), '但 owner 确实在 emb-a 的子树里')
  const shouldReset = t.isOwnedBySceneSubtree(PATH_A)
  assert(shouldReset, '父 Scene 退场时 owner 属于自己子树 → 清 tracker')
}

// 27.3 根 Scene 退场 → 无条件清（sceneDepth === 0 走这一条）
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A1, 0, 0)
  const shouldReset = true // sceneDepth === 0
  assert(shouldReset, '根 Scene 整页退场无条件清空')
  assert(t.isOwnedBySceneSubtree(PATH_ROOT), '任何 owner 都算根的子树（前缀长度 0）')
}

// 27.4 子树前缀不能跨兄弟：owner 在 emb-a，emb-b 的 Scene 退场不清
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  assert(!t.isOwnedBySceneSubtree(PATH_B), '兄弟 emb-b 不算 emb-a 的子树')
}

// 27.5 owner 在根，任意子 Scene 退场都不清（根层手势不能被子层卸载打断）
{
  const t = createGestureStateTracker()
  t.beginPinch(0, 0)
  assert(!t.isOwnedBySceneSubtree(PATH_A), 'pinch 的 owner 是根，不在子层子树里')
  assert(t.isPinching(), '根层 pinch 不会被无关子 Scene 卸载打断')
}

// 复现 Node / Embed 那个 PanGesture 的双分支收尾。
// 这一个 handler 起手有两条路：菜单选了"移动"走 move，否则走 panCanvas 拖画布。
// 收尾若固定当成 move 收，真走 panCanvas 时 owns 永远是 false，tracker 卡在 panCanvas。
function finishNodePanLike(tracker, localGestureMode, nodeId, cancelled) {
  if (localGestureMode === 'moveNode') {
    const state = tracker.getState()
    const owns = state.mode === 'moveNode' && state.activeItemId === nodeId &&
      tracker.isActiveItemInScene(PATH_A)
    if (owns) {
      if (!cancelled) { savedNodes.push(nodeId) }
      tracker.reset()
    }
    return 'moveNode'
  }
  tracker.clearIfOwnedBy('panCanvas', PATH_A)
  return 'panCanvas'
}

const savedNodes = []

console.log('')
console.log('28. 时序回归 ⑧：Pan 收尾必须按本地起手模式分流，不能固定当成 move')

// 28.1 老写法：把 panCanvas 的收尾当成 moveNode 收 → tracker 卡死
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)   // 本层 gestureMode = 'panCanvas'
  // 老写法：finishOwnedMove('moveNode', ...) 固定按 move 收
  const state = t.getState()
  const owns = state.mode === 'moveNode' && state.activeItemId === 'n-a1'
  assert(!owns, '老写法：真走 panCanvas 时按 moveNode 收 → owns=false')
  // 不 reset，tracker 就卡在这儿
  assert(t.getState().mode === 'panCanvas', '老写法之后 tracker 卡在 panCanvas（回归点）')
  assert(!t.isIdle(), '卡住的 tracker 挡住后续所有层的手势')
}

// 28.2 新写法：Node 上起手 panCanvas → End → tracker 回到 idle
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  const branch = finishNodePanLike(t, 'panCanvas', 'n-a1', false)
  assert(eq(branch, 'panCanvas'), '本地 gestureMode=panCanvas → 走 panCanvas 分支')
  assert(t.isIdle(), 'Node 拖画布后 tracker 回到 idle（不再卡在 panCanvas）')
}

// 28.3 Embed 同理，Cancel 也要回 idle
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_B, 0, 0)
  t.clearIfOwnedBy('panCanvas', PATH_B)
  assert(t.isIdle(), 'Embed 拖画布被取消后 tracker 回到 idle')
}

// 28.4 Node 移动：End 要落盘 + 回 idle
{
  const t = createGestureStateTracker()
  t.beginMoveNode(PATH_A, 'n-a1', 0, 0)
  const branch = finishNodePanLike(t, 'moveNode', 'n-a1', false)
  assert(eq(branch, 'moveNode'), '本地 gestureMode=moveNode → 走 moveNode 分支')
  assert(eq(savedNodes.length, 1), '移动成功要落盘一次')
  assert(eq(savedNodes[0], 'n-a1'), '落盘的是被移动的那个节点')
  assert(t.isIdle(), '移动结束后 tracker 回到 idle')
}

// 28.5 Node 移动被取消：不落盘，但 tracker 照样要回 idle
{
  const t = createGestureStateTracker()
  t.beginMoveNode(PATH_A, 'n-a1', 0, 0)
  const before = savedNodes.length
  finishNodePanLike(t, 'moveNode', 'n-a1', true)
  assert(eq(savedNodes.length, before), '取消的移动不落盘')
  assert(t.isIdle(), '取消移动后 tracker 回到 idle')
}

// 28.6 关键时序：panCanvas → pinch → 旧 Pan 的 End 到达
// 分流看本地 gestureMode（起手时写下的），pinch 仍须存活
{
  const t = createGestureStateTracker()
  t.beginPanCanvas(PATH_A, 0, 0)
  t.beginPinch(0, 0)
  finishNodePanLike(t, 'panCanvas', 'n-a1', false)
  assert(t.isPinching(), '旧 panCanvas 的 End 到达后 pinch 仍然活着')
  assert(t.getState().mode === 'pinch', '全局 mode 仍是 pinch')
}

function mkEdge(edgeId, x1, y1, x2, y2) {
  return {
    edgeId, startX: x1, startY: y1, endX: x2, endY: y2,
    arrowTipX: x2, arrowTipY: y2,
    labelX: (x1 + x2) / 2, labelY: y1 - 8, label: edgeId
  }
}

// 复现 handleBlankTap：同一个递归命中结果，同时驱动选中 edge 和清空选中。
// 边不是另写一套算法，就是消费 resolveRecursiveHit 的结果。
function handleBlankTapLike(sel, rootCtx, screenX, screenY) {
  const hit = resolveRecursiveHit(rootCtx, screenX, screenY)
  if (hit.target === null) {
    sel.clear()
    return null
  }
  sel.select(hit.target.scenePath, hit.target.objectKind, hit.target.objectId)
  return hit.target
}

console.log('')
console.log('29. 评审回归 ⑨：边能被选中，且不会抢走节点的点击')

// 29.1 根层的边：能被命中成 edge
{
  const tree = makeTree()
  tree.edges = [mkEdge('edge-root', 250, 150, 350, 150)]
  const ctx = buildRecursiveSceneContext(tree)

  const onEdge = resolveRecursiveHit(ctx, 300, 150)
  assert(onEdge.target !== null, '根层边被命中')
  assert(eq(onEdge.target.objectKind, 'edge'), 'objectKind 是 edge（此前类型里根本没有 edge）')
  assert(eq(onEdge.target.objectId, 'edge-root'), 'objectId 是边 id')
  assert(eq(describeScenePath(onEdge.target.scenePath), 'root'), '边身份带完整 scenePath')
}

// 29.2 容差内命中、容差外不算
{
  const tree = makeTree()
  tree.edges = [mkEdge('edge-root', 250, 150, 350, 150)]
  const ctx = buildRecursiveSceneContext(tree)

  assert(resolveRecursiveHit(ctx, 300, 159).target !== null, '容差内的点仍算命中边')
  assert(resolveRecursiveHit(ctx, 300, 200).target === null, '容差外的点不算边')
}

// 29.3 子层的边：拿到的必须是子层 scenePath，不能是根层
{
  const tree = makeTree()
  tree.getChildEmbeds()[0].edges = [mkEdge('edge-a', 100, 160, 220, 160)]
  const ctx = buildRecursiveSceneContext(tree)

  const hit = resolveRecursiveHit(ctx, 490, 90)
  assert(hit.target !== null, '子层边被命中')
  assert(eq(hit.target.objectKind, 'edge'), '子层边的 kind 也是 edge')
  assert(eq(hit.target.objectId, 'edge-a'), '子层边的 objectId 是子层自己的边 id')
  assert(eq(describeScenePath(hit.target.scenePath), 'root/embed:emb-a'),
    '子层边带的是子层 scenePath，不是根层')
}

// 29.4 边压在节点上时，节点优先 —— 这是 #813 以来最怕的回归
{
  const tree = makeTree()
  // 一条边从 n-root 中间穿过去
  tree.edges = [mkEdge('edge-over', 20, 40, 140, 40)]
  const ctx = buildRecursiveSceneContext(tree)

  const hit = resolveRecursiveHit(ctx, 50, 40)
  assert(hit.target !== null, '压在节点上的点被命中')
  assert(eq(hit.target.objectKind, 'node'), '节点优先于边：边不能抢走节点的点击')
  assert(eq(hit.target.objectId, 'n-root'), '命中的是节点而不是那条边')
}

// 29.5 真的空白 → target=null；且 blank tap 要把已有选中清掉
{
  const tree = makeTree()
  tree.edges = [mkEdge('edge-root', 250, 150, 350, 150)]
  const ctx = buildRecursiveSceneContext(tree)
  const sel = createSelectionState()

  // 先选中那条边
  handleBlankTapLike(sel, ctx, 300, 150)
  assert(sel.hasSelection(), '点边之后有选中')
  assert(eq(sel.selectedKind, 'edge'), '选中的 kind 是 edge')
  assert(eq(sel.selectedItemId, 'edge-root'), '选中的 id 是那条边')

  // 再点真正的空白 → 必须清空（此前 clearSelection 定义了但没有调用点）
  const cleared = handleBlankTapLike(sel, ctx, 300, 220)
  assert(cleared === null, '空白处没有目标')
  assert(!sel.hasSelection(), '点空白之后选中被清掉（此前根本不会清）')
}

// 29.6 选中子层的边时，带的是子层 scenePath
{
  const tree = makeTree()
  tree.getChildEmbeds()[0].edges = [mkEdge('edge-a', 100, 160, 220, 160)]
  const ctx = buildRecursiveSceneContext(tree)
  const sel = createSelectionState()

  handleBlankTapLike(sel, ctx, 490, 90)
  assert(eq(describeScenePath(sel.selectedScenePath), 'root/embed:emb-a'),
    '选中子层边时选中态带子层 scenePath')
  assert(eq(sel.selectedKind, 'edge'), '选中态 kind 是 edge')
  // 根层同 id 的边不存在，isSelected 必须按 scenePath+kind+itemId 三者一起比
  assert(!sel.isSelected([], 'edge', 'edge-a'), '不同 scenePath 的同 id 不算选中')
}

// 29.7 hitTestEdge 本身：重叠的边取先遍历到的那条
{
  const near = mkEdge('edge-near', 250, 150, 350, 150)
  const far = mkEdge('edge-far', 250, 140, 350, 140)
  assert(eq(hitTestEdge([near, far], 300, 150, EDGE_HIT_TOLERANCE_VP).edgeId, 'edge-near'),
    '更近的边命中')
  assert(eq(hitTestEdge([far, near], 300, 150, EDGE_HIT_TOLERANCE_VP).edgeId, 'edge-near'),
    '重叠时与遍历顺序无关，取真正更近的那条')
  assert(hitTestEdge([], 300, 150, EDGE_HIT_TOLERANCE_VP) === null, '没有边时返回 null')
}

console.log('')
console.log('30. 评审回归 ⑩：connect 被 pinch 接管后，connect_end 不能缺')

// 复现 closeSupersededConnectIfNeeded：
// 「能不能 reset 全局 tracker」和「能不能给自己旧 connect 写结束日志」是两回事。
function closeSupersededConnectIfNeededLike(tracker, connectSourcePath, logEnd) {
  if (connectSourcePath === null) { return null }
  if (tracker.getState().mode !== 'pinch') { return null }
  logEnd.push({ sourcePath: connectSourcePath, reason: 'superseded_by_pinch' })
  return null
}

// finishOwnedConnect / cancelOwnedConnect 共用的收尾分支
function finishConnectLike(tracker, connectSourcePath, logEnd) {
  const cleared = tracker.clearIfOwnedBy('connect', connectSourcePath.scenePath)
  if (cleared) {
    tracker.reset()
    logEnd.push({ sourcePath: connectSourcePath, reason: 'completed' })
    return
  }
  closeSupersededConnectIfNeededLike(tracker, connectSourcePath, logEnd)
}

const SRC = { starmapId: 'sm-root', segments: [], target: { type: 'node', nodeId: 'n-root' } }

// 30.1 旧 connect 的 End 迟到：pinch 必须活着，且要补一条 superseded_by_pinch
{
  const t = createGestureStateTracker()
  const logEnd = []
  let sourcePath = SRC
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  t.beginPinch(0, 0)
  // pinch 接管后，旧 connect 的 End 才到
  finishConnectLike(t, sourcePath, logEnd)

  assert(t.isPinching(), 'pinch 仍然存活（被接管的旧 connect 绝不能 reset 它）')
  assert(eq(logEnd.length, 1), '补了一条 connect_end')
  assert(eq(logEnd[0].reason, 'superseded_by_pinch'), '原因是 superseded_by_pinch 而不是 completed')
  sourcePath = closeSupersededConnectIfNeededLike(t, sourcePath, logEnd)
}

// 30.2 Cancel 路径同样补一条
{
  const t = createGestureStateTracker()
  const logEnd = []
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  t.beginPinch(0, 0)
  // clearIfOwnedBy('connect') 返回 false → 走 superseded 分支
  const cleared = t.clearIfOwnedBy('connect', PATH_A)
  if (!cleared) { closeSupersededConnectIfNeededLike(t, SRC, logEnd) }

  assert(!cleared, 'connect 已经不归自己了')
  assert(t.isPinching(), 'Cancel 迟到也不能动 pinch')
  assert(eq(logEnd.length, 1), 'Cancel 路径也补了一条 connect_end')
  assert(eq(logEnd[0].reason, 'superseded_by_pinch'), 'Cancel 路径的原因同样是 superseded_by_pinch')
}

// 30.3 只记一次：迟到的 End 和迟到的 Cancel 都到，只应有一条
{
  const t = createGestureStateTracker()
  const logEnd = []
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  t.beginPinch(0, 0)
  // 模拟源码：把 connectSourcePath 置空，保证只会记一次
  let sourcePath = SRC
  sourcePath = closeSupersededConnectIfNeededLike(t, sourcePath, logEnd)
  sourcePath = closeSupersededConnectIfNeededLike(t, sourcePath, logEnd)

  assert(eq(logEnd.length, 1), 'End 和 Cancel 都迟到时也只记一条')
  assert(sourcePath === null, '记完就把 connectSourcePath 置空')
}

// 30.4 没被 pinch 接管时，不该写 superseded
{
  const t = createGestureStateTracker()
  const logEnd = []
  t.beginConnect(PATH_A, 'n-a1', 0, 0)
  closeSupersededConnectIfNeededLike(t, SRC, logEnd)

  assert(eq(logEnd.length, 0), '没有 pinch 接管时不该写 superseded_by_pinch')
  assert(t.isConnecting(), '自己的 connect 还在，不该被误清')
}

console.log('')
console.log('31. 评审回归 ⑪：边不是连线的合法端点，不能拿 edgeId 去建边')

// 复现 finishConnect 的端点收窄。老写法把 edge 落进 else 分支当成 node。
function buildConnectTargetLike(rootCtx, screenX, screenY) {
  const hit = resolveRecursiveHit(rootCtx, screenX, screenY)
  if (hit.target === null) { return { rejected: 'blank_target' } }
  if (hit.target.objectKind === 'edge') { return { rejected: 'edge_target' } }
  return {
    kind: hit.target.objectKind === 'embed' ? 'embed' : 'node',
    itemId: hit.target.objectId
  }
}

// 31.1 根层的边：从节点拉线松手落在边命中范围内 → 必须被拒
{
  const tree = makeTree()
  tree.edges = [mkEdge('edge-root', 250, 150, 350, 150)]
  const ctx = buildRecursiveSceneContext(tree)

  const result = buildConnectTargetLike(ctx, 300, 150)
  assert(eq(result.rejected, 'edge_target'), '根层边被拒绝为连线端点')
  assert(result.kind === undefined, '不会产出 kind=node 的端点')
}

// 31.2 老写法会产出什么：拿 edgeId 冒充 nodeId
{
  const tree = makeTree()
  tree.edges = [mkEdge('edge-root', 250, 150, 350, 150)]
  const ctx = buildRecursiveSceneContext(tree)
  const hit = resolveRecursiveHit(ctx, 300, 150)

  const legacyKind = hit.target.objectKind === 'embed' ? 'embed' : 'node'
  assert(eq(legacyKind, 'node'), '老写法：edge 落进 else 分支被当成 node（回归点）')
  assert(eq(hit.target.objectId, 'edge-root'), '老写法：edgeId 被原样当成了 nodeId')
}

// 31.3 子层的边同样必须被拒
{
  const tree = makeTree()
  tree.getChildEmbeds()[0].edges = [mkEdge('edge-a', 100, 160, 220, 160)]
  const ctx = buildRecursiveSceneContext(tree)

  assert(eq(buildConnectTargetLike(ctx, 490, 90).rejected, 'edge_target'),
    '子层边也被拒绝为连线端点')
}

// 31.4 拒绝边之后，节点和 Embed 仍然是合法端点（别把口子收过头）
{
  const tree = makeTree()
  tree.edges = [mkEdge('edge-root', 250, 150, 350, 150)]
  const ctx = buildRecursiveSceneContext(tree)

  assert(eq(buildConnectTargetLike(ctx, 50, 40).itemId, 'n-root'), '节点仍是合法端点')
  assert(eq(buildConnectTargetLike(ctx, 500, 20).kind, 'embed'), 'Embed 仍是合法端点')
  assert(eq(buildConnectTargetLike(ctx, 300, 220).rejected, 'blank_target'), '真空白仍是 blank_target')
}

console.log('')
console.log('32. 评审回归 ⑫：选中一变，所有已注册的 Scene 都要重画边')

// 复现 redrawAllSceneEdges：共享选中是整棵树唯一一份。
const redraws = []
const registry = createSceneRegistry()
function regScene(scenePath, starmapId) {
  return createSceneNode(scenePath, starmapId, {
    redrawEdgesFn() { redraws.push(describeScenePath(scenePath)) }
  })
}
registry.register(regScene([], 'sm-root'))
registry.register(regScene(PATH_A, 'sm-a'))
registry.register(regScene(PATH_B, 'sm-b'))
const selection = createSelectionState()

// 32.1 root + childA + childB 都在册时，redrawAllEdges 要打到三层
{
  assert(eq(registry.size(), 3), '注册了 root / childA / childB 三层')
  redraws.length = 0
  registry.redrawAllEdges()
  assert(eq(redraws.length, 3), '一次重画打到全部三层')
  assert(eq(redraws[0], 'root'), '包含根层')
  assert(eq(redraws[1], 'root/embed:emb-a'), '包含子层 A')
  assert(eq(redraws[2], 'root/embed:emb-b'), '包含子层 B')
}

// 32.2 选中子层 A 的边：子层 A 该按 selected 画，其余层不该
{
  selection.select(PATH_A, 'edge', 'edge-a')
  redraws.length = 0
  registry.redrawAllEdges()
  assert(eq(redraws.length, 3), '选子层边也要三层全重画（旧高亮在别的层）')

  // 重画后 childA 认这条高亮，别的层不认
  assert(selection.isSelected(PATH_A, 'edge', 'edge-a'), 'childA 按 selected 画这条边')
  assert(!selection.isSelected([], 'edge', 'edge-a'), '根层不把它画成选中')
  assert(!selection.isSelected(PATH_B, 'edge', 'edge-a'), 'childB 不把它画成选中')
}

// 32.3 改选根层节点：childA 必须不再按选中边画，而且它也得收到重画
{
  selection.select([], 'node', 'n-root')
  redraws.length = 0
  registry.redrawAllEdges()
  assert(eq(redraws.length, 3), '改选根层节点也是三层全重画')
  assert(!selection.isSelected(PATH_A, 'edge', 'edge-a'), 'childA 不再按选中边画（高亮被擦掉）')
  assert(selection.isSelected([], 'node', 'n-root'), '根层按选中节点画')
}

// 32.4 清空选中同样要三层重画
{
  selection.clear()
  redraws.length = 0
  registry.redrawAllEdges()
  assert(eq(redraws.length, 3), '清空选中也要三层全重画')
  assert(!selection.hasSelection(), '确实清空了')
}

// 32.5 已注销的层不该再被重画（否则回调打在已销毁的 Canvas 上）
{
  registry.unregister(PATH_B)
  redraws.length = 0
  registry.redrawAllEdges()
  assert(eq(redraws.length, 2), '注销后只剩两层重画')
  assert(!redraws.includes('root/embed:emb-b'), '已注销的层不再被重画')
}

console.log('')
console.log('33. 评审回归 ⑬：同距离的边取先遍历到的那条')
{
  const first = mkEdge('first', 0, 0, 100, 0)
  const second = mkEdge('second', 0, 0, 100, 0)
  assert(eq(hitTestEdge([first, second], 50, 0, EDGE_HIT_TOLERANCE_VP).edgeId, 'first'),
    '同距离时取先遍历到的（写成 <= 会让后面的覆盖前面的）')
  assert(eq(hitTestEdge([second, first], 50, 0, EDGE_HIT_TOLERANCE_VP).edgeId, 'second'),
    '交换顺序则取另一条：规则是"先遍历到的"，不是"某条固定的"')
  const near = mkEdge('near', 250, 150, 350, 150)
  const far = mkEdge('far', 250, 140, 350, 140)
  assert(eq(hitTestEdge([far, near], 300, 150, EDGE_HIT_TOLERANCE_VP).edgeId, 'near'),
    '不同距离时仍然取更近的，与遍历顺序无关')
}

console.log('')
console.log('34. 屏幕侧热区口径：标题 / 边框的手指宽度不随递归层数缩水（#818 复审）')

// 24/12 是**屏幕 vp 规格**。命中测试却是在本 Scene 的画布坐标里跑的，
// 所以必须先除以本 Scene 的累计比例；否则第 2 层的可点标题就只剩几 vp。
{
  const radius = DEFAULT_EMBED_DIAMETER / 2

  // 第一层：累计比例 1 → 画布口径就是屏幕口径
  const m1 = embedHitMetricsForScene(1, radius)
  assert(near(m1.titleHitHeight, EMBED_TITLE_HIT_HEIGHT) &&
    near(m1.borderHitWidth, EMBED_BORDER_HIT_WIDTH),
  '第 1 层：画布坐标就是屏幕坐标，24 / 12 原样用')

  // 第二层：画布被压到 0.5 → 命中判定用的宽度要翻倍，换到屏幕上才是 24 / 12
  const m2 = embedHitMetricsForScene(0.5, radius)
  assert(near(m2.titleHitHeight, 48) && near(m2.borderHitWidth, 24),
  '第 2 层：画布口径翻倍（24/0.5、12/0.5）')
  assert(near(m2.titleHitHeight * 0.5, EMBED_TITLE_HIT_HEIGHT) &&
    near(m2.borderHitWidth * 0.5, EMBED_BORDER_HIT_WIDTH),
  '第 2 层：乘回屏幕口径仍是 24 / 12，手指宽度没有缩水')

  // 第 10 层：同理，且不会因为除以很小而爆掉
  const m10 = embedHitMetricsForScene(0.25, radius)
  assert(near(m10.titleHitHeight * 0.25, EMBED_TITLE_HIT_HEIGHT) &&
    near(m10.borderHitWidth * 0.25, EMBED_BORDER_HIT_WIDTH),
  '第 10 层：手指宽度依旧稳定在屏幕 24 / 12')

  // 不能反过来吃掉整个圆：命中带最多就是半径
  assert(near(embedHitMetricsForScene(0.01, radius).titleHitHeight, radius) &&
    near(embedHitMetricsForScene(0.01, radius).borderHitWidth, radius),
  '热区被夹住，不会吃掉整个 innerContent（否则一点就永远是标题）')

  // 比例非法时退回 1，不能返回 NaN / Infinity
  const bad = embedHitMetricsForScene(0, radius)
  assert(near(bad.titleHitHeight, EMBED_TITLE_HIT_HEIGHT) && near(bad.borderHitWidth, EMBED_BORDER_HIT_WIDTH),
    '累计比例 <= 0 时退回 1，命中判定不会整体失效')
  const noRadius = embedHitMetricsForScene(1, 0)
  assert(noRadius.titleHitHeight === 0 && noRadius.borderHitWidth === 0,
    '半径还没量出来时热区为 0，交给上层默认命中')

  // 端到端：第 2 层里，屏幕上距离圆顶 18vp 的点判标题、距离圆顶 30vp 的点判内部。
  // 命中测试吃的是画布坐标，所以先把屏幕点按本层累计比例换算过去。
  const sceneScale = 0.5
  const rects = [{ nodeId: 'e1', x: 0, y: 0, width: DEFAULT_EMBED_DIAMETER, height: DEFAULT_EMBED_DIAMETER, radius: 100 }]
  const embedIds = new Set(['e1'])
  const toCanvas = (screenY) => screenToCanvas(100, screenY, sceneScale, 0, 0).y
  assert(hitTestWithScene(rects, 100, toCanvas(18), PATH_A, embedIds, sceneScale).hitRegion === 'title',
    '第 2 层：屏幕距圆顶 18vp 仍在标题热区内（画布上 36 <= 48）')
  assert(hitTestWithScene(rects, 100, toCanvas(30), PATH_A, embedIds, sceneScale).hitRegion === 'innerContent',
    '第 2 层：屏幕距圆顶 30vp 已经进内部（画布上 60 > 48）')

  // 老写法（固定 24/12 不除 scale）把热区在屏幕上缩成了 24 * 0.5 = 12vp
  assert(hitTestWithScene(rects, 100, toCanvas(18), PATH_A, embedIds, 1).hitRegion === 'innerContent',
    '对照：不除累计比例的话第 2 层标题带在屏幕上只剩 12vp，18vp 处就掉进内部 —— 这就是"越来越难选中"')
}

console.log('')
console.log('35. 输入节点 id 与 onChildTouchTest：命中的子图继续参加手势竞争（#818 复审）')

// onChildTouchTest 只会看到**显式命名**的节点，所以递归层必须有稳定 id。
{
  // id 带上 scenePath：同一个 instanceId 出现在不同深度也不会互相顶掉
  const idA = describeEmbedInputNodeId(PATH_A, 'emb-a1')
  const idA1 = describeEmbedInputNodeId(PATH_A1, 'emb-a1')
  assert(isEmbedInputNodeId(idA) && isEmbedInputNodeId(idA1),
    'Embed 输入节点 id 带前缀，onChildTouchTest 才认得出')
  assert(idA !== idA1,
    '同一 instanceId 在两层不共用 id：父层转发到的必须是本次实际命中的那个 child')
  assert(eq(extractEmbedInstanceIdFromInputNodeId(PATH_A, idA), 'emb-a1'),
    '能从 id 反解回 instanceId（父级要靠它定位是哪一个 Embed）')
  assert(extractEmbedInstanceIdFromInputNodeId(PATH_A, idA1) === null,
    '别的层的 id 反解不出来，父层不会把事件转给没命中的 child')
  assert(isEmbedInputNodeId(describeSceneInputNodeId(PATH_A)),
    'Scene 自己的输入节点 id 也用同一前缀，递归链首尾一致')

  // routeChildTouchTest 的镜像（#818 复审修正版）：
  // onChildTouchTest 挂在单颗 Embed 自己的外层 Stack 上，父组件就是那颗已经
  // .position() 过的 Embed Stack，所以坐标一律取 child.x / child.y（相对子组件原点），
  // 半径取 child.rect。再减一次 getEmbedScreenX/Y 就是把原点扣两遍。
  const FORWARD_COMPETITION = 1
  const DEFAULT = 0
  const mkTouchInfo = (nodeId, x, y, width = DEFAULT_EMBED_DIAMETER, height = DEFAULT_EMBED_DIAMETER) => ({
    id: nodeId, x, y, rect: { x: 0, y: 0, width, height }
  })
  const shouldForwardTouchToChild = (child) => {
    if (child === undefined || child === null || child.rect === undefined || child.rect === null) return false
    const rectWidth = child.rect.width
    const rectHeight = child.rect.height
    if (!Number.isFinite(rectWidth) || !Number.isFinite(rectHeight) || rectWidth <= 0 || rectHeight <= 0) return false
    const radius = Math.min(rectWidth, rectHeight) / 2
    const localX = child.x
    const localY = child.y
    if (!Number.isFinite(localX) || !Number.isFinite(localY)) return false
    const metrics = embedHitMetricsForScene(1, radius)
    const dx = localX - radius
    const dy = localY - radius
    const dist = Math.sqrt(dx * dx + dy * dy)
    if (dist > radius) return false
    if (localY <= metrics.titleHitHeight) return false
    if (dist >= radius - metrics.borderHitWidth) return false
    return true
  }
  const routeChildTouchTest = (children, scenePath) => {
    if (children === undefined || children === null || children.length === 0) {
      return { strategy: DEFAULT }
    }
    for (const child of children) {
      if (child === undefined || child === null) continue
      // instanceId 只用来确认"这是当前 Scene 自己的 Embed 内壳节点"，不参与算坐标
      const instanceId = extractEmbedInstanceIdFromInputNodeId(scenePath, child.id)
      if (instanceId === null) continue
      if (shouldForwardTouchToChild(child)) return { strategy: FORWARD_COMPETITION, id: child.id }
    }
    return { strategy: DEFAULT }
  }

  const id = describeEmbedInputNodeId(PATH_A, 'emb-a1')
  const at = (x, y) => [mkTouchInfo(id, x, y)]
  const inner = routeChildTouchTest(at(100, 120), PATH_A)
  assert(inner.strategy === FORWARD_COMPETITION && inner.id === id,
    '点在子星图内部：FORWARD_COMPETITION 转给命中的 child，子层单指手势继续能赢')
  const title = routeChildTouchTest(at(100, 10), PATH_A)
  assert(title.strategy === DEFAULT,
    '点在标题带：不转发给 child，交给 onTouchIntercept Block（这一步是"选中这个子星图"）')
  const border = routeChildTouchTest(at(100, 100 - (100 - 5)), PATH_A)
  assert(border.strategy === DEFAULT,
    '点在圆环：不转发给 child，同样只选中这个子星图')
  const outside = routeChildTouchTest(at(500, 500), PATH_A)
  assert(outside.strategy === DEFAULT,
    '点在圆外：不转发，事件落回父星图')
  assert(routeChildTouchTest([mkTouchInfo('some_other_component', 100, 120)], PATH_A).strategy === DEFAULT,
    '命中的是非 Embed 输入节点：不转发，别人的组件不归这条链管')

  // ── 回归：#818 复审点名的"原点扣两遍" ──
  // ArkUI 里 TouchTestInfo.x/y 已经是相对**子组件**左上角的。真实场景里 child.x/y
  // 与 Embed 在父 Scene 里的位置完全无关——它只管自己壳内部。
  // 下面这个用例在旧写法（parentX/parentY 再减一次 getEmbedScreenX/Y）下必然失败：
  // Embed 摆在父 Scene 的 (300,200)，手指点在圆心 → 旧写法算出 (-200,-100) → 圆外 → 不转发。
  assert(near(shouldForwardTouchToChild(mkTouchInfo(id, 100, 120)), true),
    '坐标只看 child.x/y：Embed 摆在父 Scene (300,200)、手指点在圆心时照样判定为"内部"')
  const legacyBug = (() => {
    // 老写法：把 child.x 当成父组件坐标，再减一次 Embed 原点 (300,200)
    const localX = 100 - 300
    const localY = 120 - 200
    const radius = DEFAULT_EMBED_DIAMETER / 2
    return Math.sqrt((localX - radius) ** 2 + (localY - radius) ** 2) > radius
  })()
  assert(legacyBug === true,
    '对照：老写法在这里算出 (-200,-100) 落在圆外 → FORWARD_COMPETITION 永远走不到（真机症状来源）')
  assert(routeChildTouchTest(at(100, 120), PATH_A).strategy === FORWARD_COMPETITION,
    '无论 Embed 摆在父 Scene 的哪个位置，内部命中都会转发')
  assert(routeChildTouchTest(at(100, 8), PATH_A).strategy === DEFAULT,
    '无论 Embed 摆在父 Scene 的哪个位置，标题带命中都不会下沉')
  // 半径来自 child.rect，不是写死 DEFAULT_EMBED_DIAMETER：缩放后 rect 变小，
  // 屏幕侧热区仍要跟着换算，不能拿常量当半径。
  const scaled = mkTouchInfo(id, 50, 30, 100, 100)
  assert(shouldForwardTouchToChild(scaled) === true,
    '半径取自 child.rect：缩到 100 之后圆心附近仍是内部')
  assert(shouldForwardTouchToChild(mkTouchInfo(id, 50, 2, 100, 100)) === false,
    '半径取自 child.rect：同一个 y=2 在 100 直径下是标题带（热区 24 > 半径 50 的一半区间）')
  assert(shouldForwardTouchToChild(mkTouchInfo(id, 10, 60, 100, 100)) === false,
    '半径取自 child.rect：贴左边 10vp 已进圆环带（12vp），不当作内部下沉')
  assert(shouldForwardTouchToChild({ id, x: 10, y: 10, rect: { x: 0, y: 0, width: 0, height: 0 } }) === false,
    'rect 还没量出来（宽高 0）时不转发，不去算半径')
  assert(shouldForwardTouchToChild({ id, x: NaN, y: 10, rect: { x: 0, y: 0, width: 200, height: 200 } }) === false,
    '坐标非有限数时不转发')
  assert(routeChildTouchTest([], PATH_A).strategy === DEFAULT,
    'children 为空 → DEFAULT，不去猜')

  // 结构守卫（#818 复审）：触摸测试这一段不能再碰 parentX/parentY 或 Embed 原点。
  // 这两样都出现在"把原点扣两遍"的旧写法里，是 FORWARD_COMPETITION 走不到的根因。
  const ttSource = readStarmapSource('ui/StarMapScene.ets')
  const predStart = ttSource.indexOf('private shouldForwardTouchToChild(')
  const routeStart = ttSource.indexOf('private routeChildTouchTest(')
  assert(predStart >= 0 && routeStart > predStart,
    'shouldForwardTouchToChild 在 routeChildTouchTest 之前，两个都在')
  const predBody = ttSource.slice(predStart, routeStart)
  assert(!predBody.includes('parentX') && !predBody.includes('parentY'),
    'shouldForwardTouchToChild 不再用 parentX/parentY（那是相对父组件，也就是这颗已 position 过的 Embed Stack）')
  assert(!predBody.includes('getEmbedScreenX(') && !predBody.includes('getEmbedScreenY('),
    'shouldForwardTouchToChild 不再减 Embed 原点（child.x/y 已经是壳内部坐标，原点只会被扣两遍）')
  assert(/child\.rect\.width/.test(predBody) && /child\.rect\.height/.test(predBody) &&
    /Math\.min\(rectWidth, rectHeight\)\s*\/\s*2/.test(predBody),
  '半径取自 child.rect，不写死 DEFAULT_EMBED_DIAMETER')
  assert(predBody.includes('const localX: number = child.x') && predBody.includes('const localY: number = child.y'),
    '判定坐标就是 child.x / child.y')
  const routeBody = ttSource.slice(routeStart, routeStart + 1600)
  assert(routeBody.includes('extractEmbedInstanceIdFromInputNodeId(this.scenePath, child.id)'),
    'routeChildTouchTest 仍靠 instanceId 确认"这是当前 Scene 的 Embed 内壳节点"')
  assert(routeBody.includes('TouchTestStrategy.FORWARD_COMPETITION'),
    '命中的 Embed 内壳走 FORWARD_COMPETITION（根的两指链不被截断，child 单指仍在竞争）')
  assert(!/shouldForwardTouchToChild\(\s*children\s*,/.test(routeBody),
    'shouldForwardTouchToChild 只收一个 TouchTestInfo，不再传整组 children + 父坐标')
  assert(!/shouldForwardTouchToChild\([^)]*parentX/.test(ttSource),
    '全文件不再有 shouldForwardTouchToChild(..., parentX) 这种调用')

  // 结构守卫：onTouchIntercept 不许再把屏幕规格乘上累计比例。
  // 只看 onTouchIntercept 这一段：title Row 的**视觉**高度乘 scale 是对的，
  // 那是"标题带在屏幕上永远 24vp"，和命中判定是两回事。
  const sceneSource = readStarmapSource('ui/StarMapScene.ets')
  const interceptStart = sceneSource.indexOf('.onTouchIntercept(')
  const interceptEnd = sceneSource.indexOf('.onChildTouchTest(')
  assert(interceptStart >= 0 && interceptEnd > interceptStart, 'Embed 外层 Stack 同时有 onTouchIntercept 与 onChildTouchTest')
  const interceptBody = sceneSource.slice(interceptStart, interceptEnd)
  assert(!/EMBED_TITLE_HIT_HEIGHT\s*\*\s*this\.viewportScaleValue\(\)/.test(interceptBody),
    'onTouchIntercept 不再把标题热区乘 viewportScaleValue（那一层坐标已经是屏幕 vp）')
  assert(!/EMBED_BORDER_HIT_WIDTH\s*\*\s*this\.viewportScaleValue\(\)/.test(interceptBody),
    'onTouchIntercept 不再把边框热区乘 viewportScaleValue')
  // #821 复审：24 / 12vp 是屏幕侧手指尺寸，组件局部坐标必须除一次累计比例。
  // UI 侧与递归命中共用同一个 embedHitMetricsForScene，不允许再传 1 当地尺寸。
  assert(interceptBody.includes('this.embedLocalHitMetrics(radius)'),
    '真实触摸分流走 embedLocalHitMetrics（累计比例口径），不再硬传 scale = 1')
  assert(!/embedHitMetricsForScene\(\s*1\s*,/.test(sceneSource),
    'UI 侧不再出现 embedHitMetricsForScene(1, ...)：那会把 24/12vp 当成本地尺寸')
  assert(sceneSource.includes('private embedLocalHitMetrics(radius: number): EmbedHitMetrics {') &&
    /private embedLocalHitMetrics\(radius: number\): EmbedHitMetrics \{\s*return embedHitMetricsForScene\(this\.viewportScaleValue\(\), radius\)/.test(sceneSource),
    'embedLocalHitMetrics 把 viewportScaleValue 交给 embedHitMetricsForScene（和 resolveRecursiveHit 同一个 scale 口径）')
  assert(predBody.includes('this.embedLocalHitMetrics(radius)'),
    'shouldForwardTouchToChild 与 onTouchIntercept 共用同一份局部热区口径')
  assert(sceneSource.includes('onChildTouchTest('),
    'Embed 外层 Stack 接了 onChildTouchTest，深层触点不会被触摸测试链截断')
}

console.log('')
console.log('')
console.log('36. 局部 fit 给父圆留出交互壳：子子星图自然小于子星图，不靠固定深度系数（#818 复审）')

const sceneSource = readStarmapSource('ui/StarMapScene.ets')

// 之前只按 圆内接正方形 fit，一颗 200 的子 Embed 能拿到 ~0.63，把父圆内部占得很满；
// 父圆自己的标题 / 边框热区没有任何余量。改成先扣掉交互壳再 fit。
{
  assert(EMBED_INTERACTION_SHELL_VP === EMBED_TITLE_HIT_HEIGHT + EMBED_BORDER_HIT_WIDTH,
    '交互壳就是"标题带 + 边框环"本身，不另造一套数字')
  const safeSide = computeEmbedInnerContentSafeSide(DEFAULT_EMBED_DIAMETER, EMBED_FIT_PADDING_VP)
  const oldSafeSide = DEFAULT_EMBED_DIAMETER * CIRCLE_INNER_SAFE_RATIO - EMBED_FIT_PADDING_VP * 2
  assert(safeSide < oldSafeSide && safeSide > 0,
    '扣掉交互壳之后可用区确实变小（141.4 - 36 - 16 ≈ 89.4），但仍是正的')
  assert(computeEmbedInnerContentSafeSide(0, EMBED_FIT_PADDING_VP) === 0 &&
    computeEmbedInnerContentSafeSide(NaN, EMBED_FIT_PADDING_VP) === 0,
    '容器尺寸还没量出来时返回 0，不返回 NaN')

  // 两层嵌套：一颗 200×200 的子星图，父圆里 fit 一遍，孙圆自然比父圆小
  const parentSafeSide = safeSide
  const parentFit = parentSafeSide / DEFAULT_EMBED_DIAMETER
  const grandchildDiameter = DEFAULT_EMBED_DIAMETER * parentFit
  const childOfGrandchildFit = computeEmbedInnerContentSafeSide(grandchildDiameter, EMBED_FIT_PADDING_VP) / grandchildDiameter
  const greatGrandchildDiameter = grandchildDiameter * childOfGrandchildFit
  assert(grandchildDiameter < DEFAULT_EMBED_DIAMETER,
    '子子星图自然小于子星图（父圆里 fit 出来的一份）')
  assert(greatGrandchildDiameter < grandchildDiameter,
    '再深一层继续自然收缩，没有任何"每层乘 0.6"之类的固定系数')

  // 每一层的可用区都和它自己的父圆一起缩，没有一处按深度写死
  const view = createSceneView({ sceneDepth: 1, inheritedScale: 1, sceneWidth: DEFAULT_EMBED_DIAMETER, sceneHeight: DEFAULT_EMBED_DIAMETER })
  view.fitView([{ nodeId: 'e', x: 0, y: 0, width: DEFAULT_EMBED_DIAMETER, height: DEFAULT_EMBED_DIAMETER, radius: 100 }])
  assert(view.fitScale > 0 && view.fitScale < 1,
    '单颗子星图铺进父圆可用区，比例由内容包围盒和可用区现算')

  // fitView / clamp / 新建三处用的是同一份安全区
  const viewportSource = readStarmapSource('platform/StarMapViewport.ets')
  const clampUsesSafeSide = viewportSource.includes('const safeSize: number = computeEmbedInnerContentSafeSide(localSceneSize, paddingVp)')
  assert(clampUsesSafeSide,
    'clampItemToEmbedSafeArea 直接调 computeEmbedInnerContentSafeSide，不另写一遍公式')
  const fitUsesSafeSide = /computeEmbedInnerContentSafeSide\(\s*Math\.min\(localWidth, localHeight\),\s*EMBED_FIT_PADDING_VP\s*\)/.test(sceneSource)
  assert(fitUsesSafeSide, 'fitView 用的也是同一份可用区')
  assert(sceneSource.includes('clampItemToEmbedSafeArea('),
    '移动 / 新建节点与子星图共用这份安全区')

  // fit 出来的内容本来就在区内：安全区是自洽的，不会"先越界再被夹回"
  const fitted = computeFittedViewport(
    computeContentBounds([{ nodeId: 'e', x: 0, y: 0, width: DEFAULT_EMBED_DIAMETER, height: DEFAULT_EMBED_DIAMETER, radius: 100 }]),
    safeSide, safeSide, 0, DEFAULT_EMBED_DIAMETER, DEFAULT_EMBED_DIAMETER)
  assert(near(fitted.zoomScale, safeSide / DEFAULT_EMBED_DIAMETER),
    'fit 比例 = 可用区边长 / 内容边长，与安全区完全对齐')
  const clampedAfterFit = clampItemToEmbedSafeArea(0, 0, DEFAULT_EMBED_DIAMETER, DEFAULT_EMBED_DIAMETER, fitted.zoomScale, fitted.offsetX, fitted.offsetY, DEFAULT_EMBED_DIAMETER, EMBED_FIT_PADDING_VP)
  assert(near(clampedAfterFit.x, 0) && near(clampedAfterFit.y, 0),
    '刚 fit 完的内容不会被安全区 clamp 挪动 —— fit 和 clamp 是同一套边界')
}

console.log('')
console.log('')
console.log('37. 视觉 LOD（#820）：投影 → 折叠档位 → 视觉焦点，纯函数不散进 build()')

// ── 被测规格：platform/StarMapVisualLod.ets ──
const INTERACTIVE_ENTER_COVERAGE = 0.70
const INTERACTIVE_EXIT_COVERAGE = 0.60
const PREVIEW_MIN_DIAMETER_VP = 48
const PREVIEW_EXIT_DIAMETER_VP = 40
const FOCUS_ENTER_COVERAGE = 0.70
const FOCUS_EXIT_COVERAGE = 0.55
const FOCUS_CENTER_ENTER_RATIO = 0.30
const FOCUS_CENTER_EXIT_RATIO = 0.20

function viewportShortSideVp(context) {
  const w = context.viewportWidthVp
  const h = context.viewportHeightVp
  if (!Number.isFinite(w) || !Number.isFinite(h) || w <= 0 || h <= 0) {
    return 0
  }
  return Math.min(w, h)
}

function effectiveOwnerScale(ownerEffectiveScale) {
  if (!Number.isFinite(ownerEffectiveScale) || ownerEffectiveScale <= 0) {
    return 1
  }
  return ownerEffectiveScale
}

function projectEmbedMetrics(ownerEffectiveScale, context) {
  const scale = effectiveOwnerScale(ownerEffectiveScale)
  const projectedDiameterVp = DEFAULT_EMBED_DIAMETER * scale
  const shortSide = viewportShortSideVp(context)
  const coverage = shortSide > 0 ? projectedDiameterVp / shortSide : 0
  return { projectedDiameterVp, coverage }
}

function scenePathDepthOf(scenePath) {
  if (scenePath === 'root') { return 0 }
  let depth = 0
  for (let i = 0; i < scenePath.length; i++) {
    if (scenePath.charAt(i) === '/') { depth += 1 }
  }
  return depth
}

function parentScenePathOf(scenePath) {
  if (scenePath === 'root') { return 'root' }
  const idx = scenePath.lastIndexOf('/embed:')
  if (idx < 0) { return 'root' }
  return scenePath.substring(0, idx)
}

function isOnFocusChain(embedScenePath, focusScenePath) {
  return embedScenePath === focusScenePath || focusScenePath.startsWith(embedScenePath + '/')
}

function resolveDeepZoomDetail(metrics, previousDetail) {
  if (!Number.isFinite(metrics.projectedDiameterVp) || !Number.isFinite(metrics.coverage)) {
    return 'shell'
  }
  if (metrics.coverage >= INTERACTIVE_ENTER_COVERAGE) {
    return 'interactive'
  }
  if (previousDetail === 'interactive' && metrics.coverage >= INTERACTIVE_EXIT_COVERAGE) {
    return 'interactive'
  }
  if (metrics.projectedDiameterVp >= PREVIEW_MIN_DIAMETER_VP) {
    return 'preview'
  }
  if (previousDetail === 'preview' && metrics.projectedDiameterVp >= PREVIEW_EXIT_DIAMETER_VP) {
    return 'preview'
  }
  return 'shell'
}

function resolveEmbedDetailForScene(metrics, embedScenePath, focusScenePath, previousDetail) {
  const detail = resolveDeepZoomDetail(metrics, previousDetail)
  if (detail === 'shell' && isOnFocusChain(embedScenePath, focusScenePath)) {
    return 'preview'
  }
  return detail
}

function inCenterWindow(candidate, ratio) {
  return candidate.centerRatioX >= ratio &&
    candidate.centerRatioX <= 1 - ratio &&
    candidate.centerRatioY >= ratio &&
    candidate.centerRatioY <= 1 - ratio
}

function inCenterEnterRegion(candidate) { return inCenterWindow(candidate, FOCUS_CENTER_ENTER_RATIO) }
function inCenterExitRegion(candidate) { return inCenterWindow(candidate, FOCUS_CENTER_EXIT_RATIO) }
function inCenterRegion(candidate) { return inCenterEnterRegion(candidate) }

function resolveFocusCandidate(candidates) {
  let best = null
  let bestDepth = -1
  let bestCoverage = -1
  for (const candidate of candidates) {
    if (!inCenterEnterRegion(candidate)) { continue }
    const depth = scenePathDepthOf(candidate.scenePath)
    if (best === null || depth > bestDepth ||
      (depth === bestDepth && candidate.coverage > bestCoverage)) {
      best = candidate
      bestDepth = depth
      bestCoverage = candidate.coverage
    }
  }
  return best
}

function shouldPromoteFocus(coverage) {
  return Number.isFinite(coverage) && coverage >= FOCUS_ENTER_COVERAGE
}

function shouldDemoteFocus(candidate) {
  return !Number.isFinite(candidate.coverage) ||
    candidate.coverage < FOCUS_EXIT_COVERAGE ||
    !inCenterExitRegion(candidate)
}

function findFocusCandidate(scenePath, candidates) {
  for (const candidate of candidates) {
    if (candidate.scenePath === scenePath) { return candidate }
  }
  return null
}

function promotableCandidates(candidates) {
  const promotable = []
  for (const candidate of candidates) {
    if (shouldPromoteFocus(candidate.coverage)) { promotable.push(candidate) }
  }
  return promotable
}

function resolveFocusScenePath(currentFocusScenePath, candidates) {
  const promotable = promotableCandidates(candidates)
  if (currentFocusScenePath === 'root') {
    const winner = resolveFocusCandidate(promotable)
    return winner !== null ? winner.scenePath : 'root'
  }
  const current = findFocusCandidate(currentFocusScenePath, candidates)
  const descendants = []
  for (const candidate of promotable) {
    if (candidate.scenePath.startsWith(currentFocusScenePath + '/')) { descendants.push(candidate) }
  }
  const deeper = resolveFocusCandidate(descendants)
  if (deeper !== null) { return deeper.scenePath }
  if (current !== null && !shouldDemoteFocus(current)) { return currentFocusScenePath }
  return parentScenePathOf(currentFocusScenePath)
}

// ── 被测规格：platform/StarMapGeometry.ets 的 collectEmbedFocusProbes ──
function collectEmbedFocusProbesInScene(root, context, viewportWidthVp, viewportHeightVp, probes) {
  for (const rect of context.rects) {
    if (!context.embedInstanceIds.has(rect.nodeId)) { continue }
    const localCenterX = rect.x + rect.width / 2
    const localCenterY = rect.y + rect.height / 2
    const localScreen = canvasToScreen(localCenterX, localCenterY,
      context.scale, context.offsetX, context.offsetY)
    const rootScreen = convertPointToRoot(root, context.scenePath, localScreen.x, localScreen.y)
    const childSegments = [
      ...cloneScenePath(context.scenePath),
      { type: 'enterEmbed', instanceId: rect.nodeId, nodeId: null }
    ]
    probes.push({
      childScenePath: describeScenePath(childSegments),
      centerRatioX: rootScreen.x / viewportWidthVp,
      centerRatioY: rootScreen.y / viewportHeightVp,
      ownerScale: context.scale
    })
  }
  for (const child of context.children.values()) {
    collectEmbedFocusProbesInScene(root, child, viewportWidthVp, viewportHeightVp, probes)
  }
}

function collectEmbedFocusProbes(root, viewportWidthVp, viewportHeightVp) {
  const probes = []
  if (viewportWidthVp <= 0 || viewportHeightVp <= 0) { return probes }
  collectEmbedFocusProbesInScene(root, root, viewportWidthVp, viewportHeightVp, probes)
  return probes
}

const viewport = (w, h, focus = 'root') => ({
  viewportWidthVp: w, viewportHeightVp: h, focusScenePath: focus
})

console.log('37a. interactive 门槛看"圆占视口短边的比例"，不看这是第几层')
{
  // 视口短边 400 时 coverage 0.70 → 投影直径 280 → 累计比例 1.4
  const context = viewport(400, 800)
  const enterScale = INTERACTIVE_ENTER_COVERAGE * 400 / DEFAULT_EMBED_DIAMETER
  assert(near(enterScale, 1.4),
    `短边 400 时进入 interactive 需要累计比例 ${enterScale.toFixed(2)}（现算，不是写死的深度系数）`)

  const atEnter = projectEmbedMetrics(enterScale, context)
  assert(resolveDeepZoomDetail(atEnter, null) === 'interactive',
    'coverage 正好等于 0.70 → interactive（>= 而不是 >）')
  assert(resolveDeepZoomDetail(projectEmbedMetrics(enterScale - 0.01, context), null) === 'preview',
    '差一点点就掉出 interactive → preview（连续判据，不是层数）')

  // 交互档滞回：0.70 ~ 0.60 之间保持上一档，不抖
  const inHysteresis = projectEmbedMetrics(0.65 * 400 / DEFAULT_EMBED_DIAMETER, context)
  assert(resolveDeepZoomDetail(inHysteresis, 'interactive') === 'interactive',
    'coverage 0.65 落在滞回窗口内 → 保持 interactive')
  assert(resolveDeepZoomDetail(inHysteresis, null) === 'preview',
    '同样的 0.65 首次进来只给 preview（滞回只对已有档位生效）')
  assert(resolveDeepZoomDetail(projectEmbedMetrics(0.59 * 400 / DEFAULT_EMBED_DIAMETER, context), 'interactive') === 'preview',
    '掉出 0.60 下沿 → 降级')

  // preview 门槛：投影直径 ≥ 48vp；滞回下沿 40vp
  const at48 = projectEmbedMetrics(PREVIEW_MIN_DIAMETER_VP / DEFAULT_EMBED_DIAMETER, context)
  assert(resolveDeepZoomDetail(at48, null) === 'preview',
    '投影直径正好 48vp → preview（>= 而不是 >）')
  const at40 = projectEmbedMetrics(PREVIEW_EXIT_DIAMETER_VP / DEFAULT_EMBED_DIAMETER, context)
  assert(resolveDeepZoomDetail(at40, null) === 'shell',
    '48vp 以下首次进来只剩 shell')
  assert(resolveDeepZoomDetail(at40, 'preview') === 'preview',
    '已经在 preview 时 40vp ~ 48vp 之间保持 preview（防抖）')
  assert(resolveDeepZoomDetail(projectEmbedMetrics(0.1, context), 'preview') === 'shell',
    '掉到 10vp → shell')

  // 投影还没算出来时退回最省的一档，不建任何交互组件
  assert(resolveDeepZoomDetail({ projectedDiameterVp: NaN, coverage: NaN }, null) === 'shell',
    '投影还没算出来（NaN）→ shell')
  assert(resolveDeepZoomDetail({ projectedDiameterVp: NaN, coverage: NaN }, 'interactive') === 'shell',
    'NaN 时连滞回也不认，不去建 child Scene')
}

console.log('')
console.log('37b. 外壳投影只乘 owner 的累计比例，不乘 Embed 自己的 child fit')
{
  const context = viewport(400, 800)
  const atRoot = projectEmbedMetrics(1, context)
  assert(near(atRoot.projectedDiameterVp, DEFAULT_EMBED_DIAMETER), '根层 Embed 外壳 = 直径 × 根比例')
  assert(near(atRoot.coverage, DEFAULT_EMBED_DIAMETER / 400),
    'coverage = 外壳投影 / 视口短边')

  // 模拟"深一层"：父层 local fit 0.4 已折进 ownerEffectiveScale。
  // 档位判定必须只吃这一个数，绝不能把 child Scene 自己的 fit 再乘一遍，
  // 否则每深一层外壳就小一截（自我实现的缩小）。
  const childOfChildFit = 0.35
  const deepWithoutChildFit = projectEmbedMetrics(0.4, context)
  const deepWrong = projectEmbedMetrics(0.4 * childOfChildFit, context)
  assert(near(deepWithoutChildFit.projectedDiameterVp, 80) &&
    near(deepWrong.projectedDiameterVp, 28),
    '错误口径会把外壳再乘一次 child fit（80 → 28）')
  assert(near(deepWithoutChildFit.projectedDiameterVp / deepWrong.projectedDiameterVp, 1 / childOfChildFit),
    '越深的 Embed 外壳越小完全来自祖先 local fit，不来自 child fit')

  // 三档门槛是可以算出来的，不是"第几层"的常量：
  // interactive = 短边 × 0.70 / 直径；preview = 48 / 直径。
  assert(near(INTERACTIVE_ENTER_COVERAGE * 400 / DEFAULT_EMBED_DIAMETER, 1.4) &&
    near(PREVIEW_MIN_DIAMETER_VP / DEFAULT_EMBED_DIAMETER, 0.24),
    'interactive / preview 门槛都是"短边比例 → 累计比例"的现算结果')
  assert(resolveDeepZoomDetail(projectEmbedMetrics(1.39, context), null) === 'preview' &&
    resolveDeepZoomDetail(projectEmbedMetrics(1.41, context), null) === 'interactive',
    'interactive 门槛两侧分别是 preview / interactive，同一份公式')

  // 深层要继续交互，只有两条路：祖先 local fit 更松，或者用户把相机放得够大。
  const depthsInteractive = (cameraScale, localFitPerLevel) => {
    const depths = []
    let scale = cameraScale
    for (let depth = 0; depth < 12; depth++) {
      if (resolveDeepZoomDetail(projectEmbedMetrics(scale, context), null) !== 'interactive') { break }
      depths.push(depth)
      scale *= localFitPerLevel
    }
    return depths
  }
  assert(depthsInteractive(1, 0.45).every(d => d < 5),
    '相机 1 时再深的层很快掉出 interactive —— "两层 / 三层"只是某个视口下的观测结果')
  assert(depthsInteractive(4, 0.45).length > depthsInteractive(1.4, 0.45).length,
    '相机放大 → 能交互的层数变多，同一个公式没有任何深度常量')
  assert(depthsInteractive(4, 0.9).length > depthsInteractive(4, 0.45).length,
    '祖先 local fit 越松 → 越深的层也够大，能交互的层数也变多')

  // 视口尺寸不直接决定外壳尺寸，但直接决定 coverage 与档位：
  const narrow = projectEmbedMetrics(1, viewport(400, 800))
  const wide = projectEmbedMetrics(1, viewport(1200, 1800))
  assert(narrow.projectedDiameterVp === wide.projectedDiameterVp && narrow.coverage > wide.coverage,
    '视口越大同样的圆 coverage 越小（分母是视口短边）')
  const narrowFocused = projectEmbedMetrics(3, viewport(400, 800))
  const wideFocused = projectEmbedMetrics(3, viewport(1200, 1800))
  assert(shouldPromoteFocus(narrowFocused.coverage) && !shouldPromoteFocus(wideFocused.coverage),
    `同一颗圆在窄视口 coverage ${narrowFocused.coverage.toFixed(2)} 能拿到焦点、宽视口 ${wideFocused.coverage.toFixed(2)} 拿不到 —— 焦点跟着视口走`)
  assert(resolveDeepZoomDetail(narrowFocused, null) === 'interactive' &&
    resolveDeepZoomDetail(wideFocused, null) === 'preview',
    '宽视口同一颗圆降档，档位纯由屏幕空间决定')

  const badViewport = projectEmbedMetrics(1, viewport(0, 0))
  assert(badViewport.coverage === 0, '视口还没量出来时 coverage = 0，不返回 NaN')
  const badScale = projectEmbedMetrics(0, context)
  assert(near(badScale.projectedDiameterVp, DEFAULT_EMBED_DIAMETER),
    '比例还没算出来时按 1 处理，不返回 0 尺寸')
}

console.log('')
console.log('37c. 焦点链上的 Embed 保底 preview，但形状永远不变')
{
  const smallMetrics = { projectedDiameterVp: 30, coverage: 0.075 }
  assert(resolveDeepZoomDetail(smallMetrics, null) === 'shell',
    '直径 30vp：普通情况下只剩圆壳')
  assert(resolveEmbedDetailForScene(smallMetrics, 'root/embed:a', 'root/embed:a', null) === 'preview',
    '焦点本身保底 preview：用户钻进去的子星图不会退化成一颗空圆')
  assert(resolveEmbedDetailForScene(smallMetrics, 'root', 'root/embed:a', null) === 'preview',
    '焦点的祖先同样保底（否则整条链会从根部断掉）')
  assert(resolveEmbedDetailForScene(smallMetrics, 'root/embed:other', 'root/embed:a', null) === 'shell',
    '焦点链之外的 Embed 仍按屏幕空间判定')
  assert(resolveEmbedDetailForScene(smallMetrics, 'root/embed:a/embed:c', 'root/embed:a', null) === 'shell',
    '焦点之后更深的一层继续按屏幕空间判定（语义缩放不会退化成"从焦点往下全保底"）')
  // 保底的只是"里面还有东西可看"，不是形状：三档都还是同一颗正圆。
  assert(near(smallMetrics.projectedDiameterVp, DEFAULT_EMBED_DIAMETER * 0.15),
    '保底前后外壳投影尺寸一模一样，档位不改几何')

  assert(scenePathDepthOf('root') === 0 &&
    scenePathDepthOf('root/embed:a') === 1 &&
    scenePathDepthOf('root/embed:a/embed:b') === 2,
    '层级按 scenePath 数，不按 sceneDepth（组件链每层会 +2）')
  assert(parentScenePathOf('root/embed:a/embed:b') === 'root/embed:a' &&
    parentScenePathOf('root') === 'root',
    '焦点掉出下沿时退回父层，不跳回根')
  assert(!isOnFocusChain('root/embed:a', 'root/embed:b'), '不同分支不同链')

  // 焦点链比对的是 candidate Embed 自己的 childScenePath，不是它所属的父 Scene。
  // 传父 Scene 的话初始 focus='root' 会把根层每一颗 Embed 都判成"在焦点链上"，
  // 根层档位直接失效；focus='root/embed:a' 时同层兄弟也会被一起撑开。
  const detailOf = (embedScenePath, focus, metrics = smallMetrics) =>
    resolveEmbedDetailForScene(metrics, embedScenePath, focus, null)
  assert(detailOf('root/embed:a', 'root/embed:a') === 'preview' &&
    detailOf('root/embed:b', 'root/embed:a') === 'shell' &&
    detailOf('root/embed:c', 'root/embed:a') === 'shell',
    'focus=root/embed:a：只有 a 被保底，同层兄弟 b / c 继续按投影尺寸正常判定')
  assert(detailOf('root/embed:a', 'root') === 'shell' &&
    detailOf('root/embed:b', 'root') === 'shell',
    'focus=root：root 下的 Embed 不能因为"所属 Scene 就是 root"被全部保底')
  assert(detailOf('root/embed:a', 'root/embed:a/embed:c') === 'preview' &&
    detailOf('root/embed:a/embed:c', 'root/embed:a/embed:c') === 'preview' &&
    detailOf('root/embed:a/embed:d', 'root/embed:a/embed:c') === 'shell',
    'focus=root/embed:a/embed:c：只保住真正的 root→a→c 路径，同层兄弟 d 不跟着保底')

  const sceneSrc821 = readStarmapSource('ui/StarMapScene.ets')
  const detailBody821 = sceneSrc821.slice(sceneSrc821.indexOf('private embedDetail('),
    sceneSrc821.indexOf('private embedDetail(') + 800)
  assert(detailBody821.includes('this.embedScenePathLabel(embedInstanceId)') &&
    !detailBody821.includes('this.scenePathLabel('),
    'embedDetail 传的是这颗 Embed 的 childScenePath，不是 owner Scene 的 scenePath')
  assert(/private embedScenePathLabel\(embedInstanceId: string\): string \{[\s\S]{0,400}type: 'enterEmbed', instanceId: embedInstanceId/.test(sceneSrc821),
    'embedScenePathLabel 按 scenePath + enterEmbed 段拼出 childScenePath')
}

console.log('')
console.log('37d. 视觉焦点：中心区域优先、再比深度、滞回不抖；只改显示参考根')
{
  const big = { scenePath: 'root/embed:big', coverage: 0.95, centerRatioX: 0.05, centerRatioY: 0.5 }
  const centeredSmall = { scenePath: 'root/embed:mid', coverage: 0.60, centerRatioX: 0.5, centerRatioY: 0.5 }
  const centeredDeeper = { scenePath: 'root/embed:mid/embed:deep', coverage: 0.58, centerRatioX: 0.52, centerRatioY: 0.48 }
  const winner = resolveFocusCandidate([big, centeredSmall, centeredDeeper])
  assert(winner !== null && winner.scenePath === 'root/embed:mid/embed:deep',
    '屏幕边缘那颗大圆不抢焦点；中心区域里取层级最深的一颗')
  assert(resolveFocusCandidate([{ scenePath: 'root/embed:a', coverage: 0.9, centerRatioX: 0.02, centerRatioY: 0.02 }]) === null,
    '全在角落时没有候选 → 焦点保持不变')
  assert(resolveFocusCandidate([{ scenePath: 'root/embed:a', coverage: 0.9, centerRatioX: 0.5, centerRatioY: 0.5 },
    { scenePath: 'root/embed:b', coverage: 0.92, centerRatioX: 0.5, centerRatioY: 0.5 }]).scenePath === 'root/embed:b',
    '同层时按覆盖率从大到小')

  assert(shouldPromoteFocus(FOCUS_ENTER_COVERAGE) && !shouldPromoteFocus(0.69),
    '进入焦点要过 0.70')
  const cand = (scenePath, coverage, centerRatioX = 0.5, centerRatioY = 0.5) =>
    ({ scenePath, coverage, centerRatioX, centerRatioY })
  assert(shouldDemoteFocus(cand('x', 0.54)) && !shouldDemoteFocus(cand('x', FOCUS_EXIT_COVERAGE)),
    '退出焦点要掉到 0.55 之下')
  assert(!shouldDemoteFocus(cand('x', 0.80, 0.5, 0.5)), '够大且还在中央 → 不退出')
  assert(shouldDemoteFocus(cand('x', 0.80, 1.2, 0.5)),
    '够大但整颗都被拖出视口（centerRatioX=1.2）→ 也要退出')
  assert(!shouldDemoteFocus(cand('x', 0.80, 0.25, 0.5)),
    '圆心 0.25 还在 20%~80% 保持窗口里 → 不退出（位置滞回）')

  // 筛选顺序：先过 70% 进入门槛，再在里面比深度。
  // 反过来（先选最深再查覆盖率）会让一颗很小的深层候选把合法父候选整批否决。
  const parentBig = cand('root/embed:a', 0.85, 0.5, 0.5)
  const childTiny = cand('root/embed:a/embed:b', 0.40, 0.5, 0.5)
  assert(resolveFocusScenePath('root', [parentBig, childTiny]) === 'root/embed:a',
    '父候选 0.85 + 深层候选 0.40 → focus 进入父候选（不能被不够大的深层候选挡死）')
  assert(resolveFocusScenePath('root', [parentBig, cand('root/embed:a/embed:b', 0.75, 0.5, 0.5)]) ===
    'root/embed:a/embed:b',
    '父 0.85 + 深层 0.75（都过 70%）→ 焦点才进入更深的那个')
  assert(resolveFocusScenePath('root', [cand('root/embed:a', 0.85, 0.5, 0.5),
    cand('root/embed:a/embed:b', 0.40, 0.25, 0.5)]) === 'root/embed:a',
    '深层候选圆心在 0.25（进入窗口外）本来就不参选，父候选照常进焦点')
  assert(resolveFocusScenePath('root', [cand('root/embed:a', 0.72, 0.25, 0.5)]) === 'root',
    '非焦点候选圆心 0.25 没进 30%~70% 进入窗口 → 不能新进入焦点')

  // 拖出视野的焦点必须退出（只看 coverage 会让它永久赖着）
  assert(resolveFocusScenePath('root/embed:a', [cand('root/embed:a', 0.80, 1.2, 0.5)]) === 'root',
    '当前 focus coverage=0.80 但 centerRatioX=1.2 → 退回父层')
  assert(resolveFocusScenePath('root/embed:a', [cand('root/embed:a', 0.80, 0.25, 0.5)]) === 'root/embed:a',
    '当前 focus coverage=0.80、centerRatioX=0.25 → 保持（还在 exit 区间里）')
  assert(resolveFocusScenePath('root/embed:a', [cand('root/embed:a', 0.80, 0.5, 0.5)]) === 'root/embed:a',
    '当前 focus 又大又在中央 → 保持')

  // 滞回窗口：0.55 ~ 0.70 之间不动，捏合停在里面不会一帧一层地抖
  assert(resolveFocusScenePath('root/embed:a', [
    { scenePath: 'root/embed:a', coverage: 0.62, centerRatioX: 0.5, centerRatioY: 0.5 }
  ]) === 'root/embed:a', '0.62 落在滞回窗口内 → 焦点不动')
  assert(resolveFocusScenePath('root/embed:a', [
    { scenePath: 'root/embed:a', coverage: 0.40, centerRatioX: 0.5, centerRatioY: 0.5 }
  ]) === 'root', '0.40 掉出下沿 → 退回父层（不是新建页面）')
  assert(resolveFocusScenePath('root', [{ scenePath: 'root/embed:a', coverage: 0.40, centerRatioX: 0.5, centerRatioY: 0.5 }]) === 'root',
    '根层没有父层可退')
  assert(resolveFocusScenePath('root/embed:a/embed:b', []) === 'root/embed:a',
    '这轮整棵树都没加载到 → 退回一层，不会一路弹回根')

  // ── 保持区间不能被祖先 / 兄弟顶掉（#820 复核第二轮）──
  assert(resolveFocusScenePath('root/embed:a/embed:b', [
    cand('root/embed:a', 0.85, 0.5, 0.5),
    cand('root/embed:a/embed:b', 0.62, 0.5, 0.5)
  ]) === 'root/embed:a/embed:b',
    '覆盖率滞回：b=0.62 在 0.55~0.70 之间 → 祖先 a=0.85 不能把它顶回父层')
  assert(resolveFocusScenePath('root/embed:a/embed:b', [
    cand('root/embed:a', 0.90, 0.5, 0.5),
    cand('root/embed:a/embed:b', 0.80, 0.25, 0.5)
  ]) === 'root/embed:a/embed:b',
    '位置滞回：b 圆心 0.25 仍在 20%~80% 保持窗口 → 祖先 a 不能顶掉它')
  assert(resolveFocusScenePath('root/embed:a/embed:b', [
    cand('root/embed:a/embed:b', 0.62, 0.5, 0.5),
    cand('root/embed:a/embed:c', 0.90, 0.5, 0.5)
  ]) === 'root/embed:a/embed:b',
    '兄弟 c=0.90 更符合进入条件，也不能在 b 合法保持时抢走焦点')
  assert(resolveFocusScenePath('root/embed:a', [
    cand('root/embed:a', 0.80, 0.5, 0.5),
    cand('root/embed:a/embed:b', 0.75, 0.5, 0.5)
  ]) === 'root/embed:a/embed:b',
    '真正更深的后代 b=0.75 满足进入条件 → 允许晋升（继续往里钻）')
  assert(resolveFocusScenePath('root/embed:a/embed:b', [
    cand('root/embed:a/embed:b', 0.40, 0.5, 0.5),
    cand('root/embed:a/embed:c', 0.90, 0.5, 0.5)
  ]) === 'root/embed:a',
    '当前焦点真失效（0.40）→ 本轮只退一层到父层，不跨分支瞬移到兄弟 c')
}

console.log('')
console.log('37e. 焦点候选只来自已实例化的 Scene，屏幕坐标从显示矩形算')
{
  const rootNode = {
    scenePath: [],
    starmapId: 'sm1',
    rects: [
      { nodeId: 'e1', x: 0, y: 0, width: 200, height: 200, radius: 100 },
      { nodeId: 'n1', x: 400, y: 400, width: 160, height: 80, radius: 16 }
    ],
    embedInstanceIds: new Set(['e1']),
    edges: [],
    scale: 1,
    offsetX: 100,
    offsetY: 200,
    getChildEmbeds: () => [{
      scenePath: [{ type: 'enterEmbed', instanceId: 'e1', nodeId: null }],
      starmapId: 'sm2',
      rects: [{ nodeId: 'e2', x: 0, y: 0, width: 100, height: 100, radius: 50 }],
      embedInstanceIds: new Set(['e2']),
      edges: [],
      scale: 0.5,
      offsetX: 10,
      offsetY: 20,
      getChildEmbeds: () => []
    }]
  }
  const probes = collectEmbedFocusProbes(buildRecursiveSceneContext(rootNode), 400, 400)
  assert(probes.length === 2, '普通节点不当焦点候选，只有 Embed 进候选')
  const rootProbe = probes.find(p => p.childScenePath === 'root/embed:e1')
  assert(near(rootProbe.centerRatioX, (100 + 100) / 400) && near(rootProbe.centerRatioY, (200 + 100) / 400),
    '圆心按画布 × 本 Scene 比例 + 累计偏移 换算到屏幕，再归一化')
  assert(near(rootProbe.ownerScale, 1), '候选带的是所属 Scene 的累计比例')
  const childProbe = probes.find(p => p.childScenePath === 'root/embed:e1/embed:e2')
  assert(childProbe !== undefined && near(childProbe.ownerScale, 0.5),
    '子 Scene 里的候选带的是子 Scene 的累计比例（相机 × 祖先 local fit）')
  // 深层圆心必须换算回根 Scene 屏幕坐标：
  // 子图内 (50,50) → 子局部屏幕 (35,45) → 扣掉父 Embed 矩形原点再乘父 scale → 父画布 (35,45)
  // → 根局部屏幕 (35×1+100, 45×1+200) = (135,245)。少了父层这段偏移就会漂到 (35,45)。
  assert(near(childProbe.centerRatioX, 135 / 400) && near(childProbe.centerRatioY, 245 / 400),
    '深层候选的圆心先 canvasToScreen 再 convertPointToRoot，父 Embed 的平移算进去了')
  assert(collectEmbedFocusProbes(buildRecursiveSceneContext(rootNode), 0, 400).length === 0,
    '视口还没量出来时没有候选（焦点保持 root）')

  // 真实递归几何：父 Embed 在根画布明显偏右，子图里的 Embed 靠左。
  // 只算子层局部偏移的话子 Embed 圆心会落在视口左侧、被误判成"屏幕边缘"，
  // 70% 的焦点规则就永远选不到它（#820 复核）。
  const deepRootNode = {
    scenePath: [],
    starmapId: 'sm1',
    rects: [{ nodeId: 'a', x: 500, y: 0, width: 200, height: 200, radius: 100 }],
    embedInstanceIds: new Set(['a']),
    edges: [],
    scale: 1,
    offsetX: 0,
    offsetY: 0,
    getChildEmbeds: () => [{
      scenePath: [{ type: 'enterEmbed', instanceId: 'a', nodeId: null }],
      starmapId: 'sm2',
      rects: [{ nodeId: 'b', x: 40, y: 40, width: 100, height: 100, radius: 50 }],
      embedInstanceIds: new Set(['b']),
      edges: [],
      scale: 1,
      offsetX: 0,
      offsetY: 0,
      getChildEmbeds: () => []
    }]
  }
  const deepProbes = collectEmbedFocusProbes(buildRecursiveSceneContext(deepRootNode), 1000, 1000)
  const deepChild = deepProbes.find(p => p.childScenePath === 'root/embed:a/embed:b')
  // 子图内圆心 (90,90) → 子局部屏幕 (90,90) → childLocalToParentCanvas 换到父画布
  // = 90/父scale + 父 Embed 矩形原点 (500,0) = (590,90) → 根局部屏幕 (590,90)。
  // 旧口径只做 × context.scale + context.offset（子层 scale=1、offset=0），
  // 算出来还是 (90,90)，父层那 500 整段丢掉。
  assert(deepChild !== undefined && near(deepChild.centerRatioX, 590 / 1000) &&
    near(deepChild.centerRatioY, 90 / 1000),
    '父 Embed 在根画布 x=500、子图内圆心 (90,90) 时，子 Embed 的根圆心是 (590,90)')
  const withoutParentOffset = (40 + 100 / 2) / 1000
  assert(!near(deepChild.centerRatioX, withoutParentOffset),
    `回归：不换算父层平移会算成 ${withoutParentOffset}，父层那 500 整个丢掉`)

  // 旧口径会误选：父 Embed 在根画布 (400,800)、子 Embed 在子图内圆心 (400,400) 时，
  // 旧口径给 (0.4,0.4) 落在中心区域内 → 有资格抢焦点；
  // 换算父层后是 (800,1200)/1000 = (0.8,1.2)，正确地被中心区域规则排除。
  const misleadingProbes = collectEmbedFocusProbes(buildRecursiveSceneContext({
    ...deepRootNode,
    rects: [{ nodeId: 'a', x: 400, y: 800, width: 200, height: 200, radius: 100 }],
    getChildEmbeds: () => [{
      scenePath: [{ type: 'enterEmbed', instanceId: 'a', nodeId: null }],
      starmapId: 'sm2',
      rects: [{ nodeId: 'b', x: 350, y: 350, width: 100, height: 100, radius: 50 }],
      embedInstanceIds: new Set(['b']),
      edges: [],
      scale: 1,
      offsetX: 0,
      offsetY: 0,
      getChildEmbeds: () => []
    }]
  }), 1000, 1000)
  const misleading = misleadingProbes.find(p => p.childScenePath === 'root/embed:a/embed:b')
  assert(near(misleading.centerRatioX, 0.8) && near(misleading.centerRatioY, 1.2),
    '父 Embed 在 (400,800) 时子 Embed 的根圆心是 (800,1200) → 比例 (0.8,1.2)')
  assert(!inCenterRegion({ centerRatioX: misleading.centerRatioX, centerRatioY: misleading.centerRatioY }),
    '旧口径的 (0.4,0.4) 会被当成"在视口中央"，换算父层后正确排除（焦点不会选到视口外的圆）')

  // 正例：父 Embed 挪到子 Embed 真正落在视口中心的位置，它就该进中心区域
  const centeredProbes = collectEmbedFocusProbes(buildRecursiveSceneContext({
    ...deepRootNode,
    rects: [{ nodeId: 'a', x: 100, y: 100, width: 200, height: 200, radius: 100 }],
    getChildEmbeds: () => [{
      scenePath: [{ type: 'enterEmbed', instanceId: 'a', nodeId: null }],
      starmapId: 'sm2',
      rects: [{ nodeId: 'b', x: 350, y: 350, width: 100, height: 100, radius: 50 }],
      embedInstanceIds: new Set(['b']),
      edges: [],
      scale: 1,
      offsetX: 0,
      offsetY: 0,
      getChildEmbeds: () => []
    }]
  }), 1000, 1000)
  const centered = centeredProbes.find(p => p.childScenePath === 'root/embed:a/embed:b')
  assert(near(centered.centerRatioX, 0.5) && near(centered.centerRatioY, 0.5),
    '父 Embed 在 (100,100) 时子 Embed 的根圆心是 (500,500) → 比例 (0.5,0.5)')
  assert(inCenterRegion({ centerRatioX: centered.centerRatioX, centerRatioY: centered.centerRatioY }),
    '圆心落在 1000×1000 视口中心区域里 → 有资格被选为焦点')

  const geometrySrc820 = readStarmapSource('platform/StarMapGeometry.ets')
  const probesSrc820 = geometrySrc820.slice(geometrySrc820.indexOf('function collectEmbedFocusProbesInScene'))
  assert(probesSrc820.includes('canvasToScreen(') && probesSrc820.includes('convertPointToRoot('),
    '焦点候选的圆心先 canvasToScreen 再 convertPointToRoot（复用递归坐标工具，不另写一套）')
  assert(/function collectEmbedFocusProbesInScene\(\s*root: RecursiveSceneContext,\s*context: RecursiveSceneContext,/.test(
    geometrySrc820.slice(geometrySrc820.indexOf('function collectEmbedFocusProbesInScene'))),
    '递归下钻时透传同一份 root，深层 Embed 才知道自己在根画布的哪')
  assert(probesSrc820.includes('centerRatioX: rootScreen.x / viewportWidthVp'),
    '比例是根 Scene 屏幕坐标除视口宽高，不是子层局部坐标')
}

console.log('')
console.log('37f. Embed 永远是那颗正圆：命中 / 边端点 / 显示几何用同一份边界')
{
  // 不再有任何"显示矩形读形状"的入口：Embed 的边界只有一个来源。
  const circleRect = { nodeId: 'e1', x: 0, y: 0, width: DEFAULT_EMBED_DIAMETER, height: DEFAULT_EMBED_DIAMETER, radius: DEFAULT_EMBED_DIAMETER / 2 }
  assert(circleRect.width === circleRect.height && circleRect.radius * 2 === circleRect.width,
    'Embed 的 rect 自证是圆（宽 = 高、半径 = 宽 / 2），不需要额外的 isCircle 标记')

  const ids = new Set(['e1'])
  const path = [{ type: 'enterEmbed', instanceId: 'e1', nodeId: null }]
  const inside = hitTestWithScene([circleRect], 100, 100, path, ids, 1)
  assert(inside !== null && inside.objectKind === 'embedInnerContent' && inside.hitRegion === 'innerContent',
    '圆内仍然是 innerContent（递归下探照旧）')
  const onRing = hitTestWithScene([circleRect], 2, 100, path, ids, 1)
  assert(onRing !== null && onRing.objectKind === 'embedBorder' && onRing.hitRegion === 'border',
    '圆环上是 border（选中这个子星图，不下探 child Scene）')
  // 圆外四个方角不算命中：矩形布局也拦不住
  assert(hitTestWithScene([circleRect], 2, 2, path, ids, 1) === null,
    '圆外方角不算命中（Embed 从来不按矩形命中）')
  assert(hitTestWithScene([circleRect], 5, 195, path, ids, 1) === null,
    '矩形框内、圆外的一点不算命中（边界只有圆，没有矩形热区）')

  // 缩放不参与命中：命中始终在画布坐标里按固定尺寸判定
  assert(hitTestWithScene([circleRect], 2, 2, path, ids, 4) === null,
    'sceneScale 放大只让标题/边框壳变薄，不把圆变成方形热区')

  const geometrySource = readStarmapSource('platform/StarMapGeometry.ets')
  assert(!geometrySource.includes('isCircularDisplayRect'),
    'Geometry 里没有"读显示矩形形状"的辅助函数了（Embed 只有圆）')
  const edgeEndpointBody = geometrySource.slice(geometrySource.indexOf('export function edgeEndpointBoundaryPoint'))
  assert(edgeEndpointBody.slice(0, 900).includes('if (isEmbed)') &&
    edgeEndpointBody.slice(0, 900).includes('lineCircleIntersection('),
    '连线端点直接判 isEmbed → 圆周；端点永远贴在用户看得见的那圈圆上')
  const hitBody = geometrySource.slice(geometrySource.indexOf('export function hitTestWithScene') === -1
    ? geometrySource.indexOf('function hitTestWithScene') : geometrySource.indexOf('export function hitTestWithScene'))
  assert(!hitBody.slice(0, 1400).includes('isCircularDisplayRect'),
    '命中路径里没有任何按显示矩形切形状的分支')

  const layoutSource = readStarmapSource('platform/StarMapLayout.ets')
  const embedLayoutBody = layoutSource.slice(layoutSource.indexOf('export function buildEmbedLayoutNodes'),
    layoutSource.indexOf('export function buildEmbedLayoutNodes') + 1200)
  assert(embedLayoutBody.includes('width: DEFAULT_EMBED_DIAMETER') &&
    embedLayoutBody.includes('height: DEFAULT_EMBED_DIAMETER') &&
    embedLayoutBody.includes('radius: DEFAULT_EMBED_DIAMETER / 2') &&
    embedLayoutBody.includes('x: embed.position.x') && embedLayoutBody.includes('y: embed.position.y'),
    'buildEmbedLayoutNodes 的位置就是 authored position，尺寸恒为默认圆（没有 displayBounds 入参）')
  assert(!embedLayoutBody.includes('displayBounds') &&
    !layoutSource.includes('EmbedDisplayBounds') && !layoutSource.includes('expandedEmbedDisplayBounds'),
    'Layout 里不再有显示边界这套东西（也不再有 isCircle 分支）')
  assert(embedLayoutBody.includes('collapsed: false'),
    'collapsed 标记恒为 false —— Embed 不再存在"折叠成节点卡"这档')
}

console.log('')
console.log('37g. 档位只换内部渲染器：外壳同一颗圆，不重建布局、不写回 Core 位置（#821）')
{
  const sceneSrc = readStarmapSource('ui/StarMapScene.ets')
  const embedItemIdx = sceneSrc.indexOf('@Builder\n  StarMapEmbedItem')
  const embedItemEnd = sceneSrc.indexOf('private buildEmbedCircleResponseRegions')
  const embedItemBody = sceneSrc.slice(embedItemIdx, embedItemEnd)
  assert(embedItemIdx > -1 && embedItemEnd > embedItemIdx,
    'StarMapEmbedItem 是唯一的 Embed 渲染入口')
  assert(!sceneSrc.includes('StarMapCollapsedEmbed') && !sceneSrc.includes('StarMapExpandedEmbed'),
    '折叠摘要卡 / 展开态两个 Builder 都没了：Embed 只有一个形状')
  assert(!sceneSrc.includes('starmap_sub_starmap'),
    '不再有"子星图摘要卡"那张矩形节点卡')

  assert(embedItemBody.includes("=== 'interactive'") && embedItemBody.includes("=== 'preview'"),
    '按 detail 档位切换内部渲染器：interactive / preview 两档各挂各的')
  assert(embedItemBody.includes('StarMapEmbedScene('), 'interactive 档才挂 child Scene')
  assert(embedItemBody.includes('StarMapEmbedPreview('), 'preview 档挂轻量 Canvas 缩略图')
  const interactiveIdx = embedItemBody.indexOf('StarMapEmbedScene(')
  const previewIdx = embedItemBody.indexOf('StarMapEmbedPreview(')
  assert(interactiveIdx > -1 && previewIdx > interactiveIdx,
    'interactive 排在 preview 前面：默认路径先判交互档')

  // preview 必须完全不参与触摸竞争：根 Scene 的两指 Pinch 拿回完整事件
  const previewBranch = embedItemBody.slice(interactiveIdx, embedItemBody.indexOf('// 2. title 视觉层'))
  const previewTail = previewBranch.slice(previewIdx)
  assert(previewTail.includes('.hitTestBehavior(HitTestMode.None)'),
    'preview 整块 HitTestMode.None，不抢也不挡根 Scene 的 Pinch')
  assert(/StarMapEmbedPreview\(\{[\s\S]{0,400}bridge: this\.bridge/.test(embedItemBody),
    'preview 拿得到 bridge：否则 aboutToAppear 直接 return，preview 永远是空圆')

  // 外壳几何只有一个来源，任何档位都一样
  assert(embedItemBody.includes('.width(DEFAULT_EMBED_DIAMETER)') &&
    embedItemBody.includes('.height(DEFAULT_EMBED_DIAMETER)') &&
    embedItemBody.includes('.borderRadius(DEFAULT_EMBED_DIAMETER / 2)'),
    '外壳尺寸恒为 DEFAULT_EMBED_DIAMETER 的正圆（不按档位换形状）')
  assert(embedItemBody.includes('.scale({ x: this.boxScale(), y: this.boxScale() })'),
    '缩放走 .scale() 显示变换，不把 boxScale 乘进 width/height')
  assert(!/\.width\(\s*\d+\s*\*\s*this\.viewportScaleValue\(\)\s*\)/.test(sceneSrc) &&
    !/\.height\(\s*\d+\s*\*\s*this\.viewportScaleValue\(\)\s*\)/.test(sceneSrc),
    'Scene 里不再有 "160 * viewportScaleValue()" 这种让布局跟着相机长大的写法')
  assert(!/fontSize\(\d+\s*\*\s*this\.viewportScaleValue\(\)\)/.test(sceneSrc),
    '字号也不再乘 viewportScaleValue（等比缩放交给 .scale）')

  assert(sceneSrc.includes('focusScenePath: string'), '档位逻辑层级按 scenePath 判定')
  assert(/@Prop @Watch\('onFocusScenePathChange'\) focusScenePath: string = 'root'/.test(sceneSrc),
    'focusScenePath 是 @Prop：焦点变化要能传到每一层 Scene（不是冻结在创建时）')
  assert(!sceneSrc.includes('embedDetailByDepth'),
    '档位判定不按 sceneDepth（组件链每层 +2，会算错层级）')
  assert(!sceneSrc.includes('refreshEmbedLodLayout'),
    '没有"切档位重建 Embed 边界"这条路径')
  const detailBody = sceneSrc.slice(sceneSrc.indexOf('private embedDetail('),
    sceneSrc.indexOf('private embedDetail(') + 800)
  assert(detailBody.includes('projectEmbedMetrics(') && detailBody.includes('resolveEmbedDetailForScene('),
    '档位来自 StarMapVisualLod 的纯函数（UI 层不自己写阈值）')
  assert(!/buildEmbedLayoutNodes\(|recomputeGeometry\(|onMoveEmbed|moveEmbed\(/m.test(detailBody),
    '算档位不重建布局、不写回 Core authored position（用户缩放不改数据）')
  assert(sceneSrc.includes('rootViewportWidth: number') && sceneSrc.includes('rootViewportHeight: number'),
    '根视口宽高由 StarMapScreen 透传进来，子 Scene 不许拿自己那个圆当视口')

  const screenSrc = readStarmapSource('ui/StarMapScreen.ets')
  assert(screenSrc.includes('@State focusScenePath: string = \'root\''),
    '视觉焦点住在 StarMapScreen（只改显示参考根，不落盘）')
  assert(screenSrc.includes('rootViewportWidth: this.canvasWidth') &&
    screenSrc.includes('rootViewportHeight: this.canvasHeight'),
    'onAreaChange 拿到的真实内容区就是根视口')
  assert(screenSrc.includes('focusScenePath: this.focusScenePath'),
    '同一份 focusScenePath 透传到每一层 Scene')
  assert(screenSrc.includes('recomputeVisualFocus()') &&
    screenSrc.includes('resolveFocusScenePath('),
    '焦点由 StarMapVisualLod 的纯函数判定，UI 层不自己写阈值')
  assert(!/build\(\)[\s\S]{0,400}focusScenePath\s*=[^=]/.test(screenSrc.slice(screenSrc.indexOf('build()'))),
    '焦点状态不在 build() 里写（不能靠重建副作用驱动显示态）')

  const embedSrc = readStarmapSource('ui/StarMapEmbedScene.ets')
  assert(embedSrc.includes('@Prop focusScenePath') &&
    embedSrc.includes('focusScenePath: this.focusScenePath'),
    'StarMapEmbedScene 原样透传 focusScenePath')
  assert(embedSrc.includes('rootViewportWidth') && embedSrc.includes('rootViewportHeight'),
    'StarMapEmbedScene 原样透传根视口宽高')
  assert(/@Prop cameraScale/.test(embedSrc) && embedSrc.includes('cameraScale: this.cameraScale'),
    '相机仍然是 @Prop 透传链，#818 的状态链没被 Deep Zoom 改坏')
  assert(/@Prop cameraOffsetX/.test(embedSrc) && /@Prop cameraOffsetY/.test(embedSrc),
    '相机偏移同样保持 @Prop')

  // preview 是独立的轻量渲染器：固定尺寸 Canvas，不注册 Scene、不挂手势
  const previewSrc = readStarmapSource('ui/StarMapEmbedPreview.ets')
  assert(previewSrc.includes('bridge: IWriterCoreBridge | null = null'),
    'preview 声明了 bridge 字段')
  assert(/async aboutToAppear\(\)[\s\S]{0,200}if \(!this\.bridge \|\| !this\.embed\.targetStarmapId\)/.test(previewSrc),
    'preview 靠 bridge 读子星图：没有 bridge 就直接不画')
  // Canvas onReady 是"画布可绘制"，不是数据变化后的重绘回调；
  // 异步 graph 后到时必须有人再画一次，否则圆里永远是空的。
  assert(previewSrc.includes('private refreshPreview(): void {') &&
    /private refreshPreview\(\): void \{\s*this\.recomputePreviewGeometry\(\)\s*if \(this\.canvasReady\) \{\s*this\.drawPreview\(\)/.test(previewSrc),
    '几何刷新与重绘收口到 refreshPreview 一个出口（graph 先到 / Canvas 先到两种时序都补得上）')
  assert(previewSrc.includes('this.childGraph = result.data') &&
    /this\.childGraph = result\.data\s*this\.refreshPreview\(\)/.test(previewSrc),
    '异步加载子 graph 成功后走 refreshPreview，而不是只算几何不重绘')
  assert(/onReady\(\(\) => \{\s*this\.canvasReady = true\s*this\.refreshPreview\(\)/.test(previewSrc),
    'onReady 也走 refreshPreview：Canvas 先 ready 时先画一张空图，graph 到了会补画')
  assert(previewSrc.includes('PREVIEW_CANVAS_SIZE') &&
    previewSrc.includes('.width(PREVIEW_CANVAS_SIZE)') && previewSrc.includes('.height(PREVIEW_CANVAS_SIZE)'),
    'preview 用固定尺寸 Canvas（不按父层比例分配缓冲区）')
  assert(previewSrc.includes('.hitTestBehavior(HitTestMode.None)'),
    'preview 不参与触摸竞争')
  assert(!/sceneRegistry|selectionState|StarMapScene\(/.test(previewSrc),
    'preview 不注册 Scene、不带选中态、不递归实例化子 Scene')
  assert(previewSrc.includes('computeContentBounds(') && previewSrc.includes('computeFittedViewport('),
    'preview 复用同一份 local fit 口径（不是另写一套缩放）')
  assert(!/\.gesture\(|Gesture\(|onTouch|onClick/.test(previewSrc),
    'preview 不挂任何手势组件（里面没有节点级交互）')

  const lodSource = readStarmapSource('platform/StarMapVisualLod.ets')
  assert(!/from '\.\/StarMapGeometry'/.test(lodSource),
    'StarMapVisualLod 不反向依赖 StarMapGeometry（会和 Viewport→Geometry 绕成初始化环）')
  assert(lodSource.includes('export type DeepZoomDetail = \'interactive\' | \'preview\' | \'shell\''),
    '三档 detail 是显式类型，不是隐含的 if/else 层级')
  for (const fn of ['projectEmbedMetrics', 'resolveDeepZoomDetail', 'resolveEmbedDetailForScene',
    'resolveFocusCandidate', 'shouldPromoteFocus', 'shouldDemoteFocus']) {
    assert(lodSource.includes(`export function ${fn}`), `${fn} 是 StarMapVisualLod 里的纯函数`)
  }
  for (const gone of ['EmbedLodMode', 'embedDisplayBoundsForLod', 'viewportDetailScale',
    'expandedEmbedSizeVp', 'resolveEmbedLod', 'outerSizeVp', 'innerUsableSizeVp']) {
    assert(!lodSource.includes(gone), `StarMapVisualLod 里不再有 ${gone}（Deep Zoom 不改几何）`)
  }
  assert(!/from '\.\/StarMapViewport'/.test(lodSource),
    '稳定圆壳之后 StarMapVisualLod 不再依赖 StarMapViewport')
  assert(!/deviceType|isTablet|isPhone|orientation|landscape/.test(lodSource),
    '档位里没有设备型号 / 横竖屏分支，层数只能是视口算出来的结果')
  assert(lodSource.includes('export const INTERACTIVE_ENTER_COVERAGE: number = 0.70') &&
    lodSource.includes('export const INTERACTIVE_EXIT_COVERAGE: number = 0.60') &&
    lodSource.includes('export const PREVIEW_MIN_DIAMETER_VP: number = 48') &&
    lodSource.includes('export const PREVIEW_EXIT_DIAMETER_VP: number = 40'),
    '两档门槛都带滞回：interactive 0.70/0.60，preview 48/40vp')

  // 焦点状态机：先过滤 70% 再比深度；退出同时看覆盖率和位置
  assert(lodSource.includes('FOCUS_CENTER_ENTER_RATIO: number = 0.30') &&
    lodSource.includes('FOCUS_CENTER_EXIT_RATIO: number = 0.20'),
    '位置滞回分进入窗口(30%)和保持窗口(20%)两个常量')
  const focusPathBody = lodSource.slice(lodSource.indexOf('export function resolveFocusScenePath'))
  assert(focusPathBody.indexOf('shouldPromoteFocus(candidate.coverage)') <
    focusPathBody.indexOf('resolveFocusCandidate(promotable)'),
    '先按覆盖率过滤出 promotable，再交给 resolveFocusCandidate 比深度')
  assert(focusPathBody.includes('promotableCandidates(candidates)'),
    'resolveFocusScenePath 走共享的 promotable 过滤，不自己内联一遍覆盖率门槛')
  assert(!/resolveFocusCandidate\(candidates\)[\s\S]{0,200}shouldPromoteFocus\(winner\.coverage\)/.test(focusPathBody),
    '不再保留"先选最深的、再查它够不够 70% 然后否决整批候选"这条路径')
  assert(focusPathBody.includes('shouldDemoteFocus(current)') &&
    focusPathBody.includes('findFocusCandidate(currentFocusScenePath, candidates)'),
    '退出判定拿的是当前焦点的完整候选（含圆心位置），不是一个孤立的 coverage 数字')
  assert(lodSource.includes('export function shouldDemoteFocus(candidate: FocusCandidate): boolean') &&
    lodSource.includes('!inCenterExitRegion(candidate)'),
    'shouldDemoteFocus 同时看 coverage 下沿和位置保持窗口')
  assert(/export function shouldDemoteFocus\(coverage: number\)/.test(lodSource) === false,
    'shouldDemoteFocus 不再只接一个 coverage 数字（那样看不到位置）')
  assert(lodSource.includes('!Number.isFinite(candidate.coverage)'),
    '非有限 coverage 仍按退出处理（不返回 NaN 焦点）')

  // 焦点转移顺序：先看后代能不能晋升，再判当前是否仍在保持区间
  const spIdx = focusPathBody.indexOf("startsWith(currentFocusScenePath + '/')")
  const holdIdx = focusPathBody.indexOf('!shouldDemoteFocus(current)')
  const parentIdx = focusPathBody.lastIndexOf('parentScenePathOf(currentFocusScenePath)')
  assert(spIdx > -1 && holdIdx > spIdx && parentIdx > holdIdx,
    'resolveFocusScenePath 的顺序是：后代晋升 → 当前保持 → 退回父层')
  assert(focusPathBody.includes("if (currentFocusScenePath === 'root')") &&
    focusPathBody.indexOf("if (currentFocusScenePath === 'root')") < spIdx,
    '根层没有需要保持的旧焦点，先正常从全体 promotable 里选')
  assert(focusPathBody.includes('resolveFocusCandidate(descendants)') &&
    !/resolveFocusCandidate\(promotable\)[\s\S]{0,400}shouldDemoteFocus\(current\)/.test(focusPathBody),
    '非根层只在"当前焦点的严格后代"里晋升，不从全树重选 winner')
  assert(lodSource.includes('function promotableCandidates(candidates: FocusCandidate[]): FocusCandidate[]'),
    'promotable 过滤抽成共用纯函数，根层与后代晋升走同一套进入门槛')

  const viewportSource = readStarmapSource('platform/StarMapViewport.ets')
  assert(!viewportSource.includes('projectLocalLengthToScreen'),
    '屏幕长度投影函数已删除：显示变换走 .scale()，不需要"把本地长度投影成屏幕长度"')
  assert(viewportSource.includes('export const CAMERA_SCALE_MIN: number = 1e-4') &&
    viewportSource.includes('export const CAMERA_SCALE_MAX: number = 1e5'),
    '相机范围放宽到数值安全边界（用户不再撞到 3× 天花板）')
  assert(!/deviceType|isTablet|isPhone|orientation|landscape/.test(viewportSource),
    'Viewport 里同样没有设备 / 横竖屏分支')
}

console.log('')
console.log('38. Deep Zoom 档位随视口重算，但外壳几何恒定（#821）')

function detailOf(embedScenePath, ownerEffectiveScale, context, previousDetail = null) {
  return resolveEmbedDetailForScene(
    projectEmbedMetrics(ownerEffectiveScale, context),
    embedScenePath, context.focusScenePath, previousDetail
  )
}

console.log('38a. 档位判据只有 coverage 和投影直径两条连续量，没有设备断点表')
{
  assert(INTERACTIVE_ENTER_COVERAGE > 0 && INTERACTIVE_EXIT_COVERAGE < INTERACTIVE_ENTER_COVERAGE &&
    INTERACTIVE_EXIT_COVERAGE > 0 && PREVIEW_MIN_DIAMETER_VP > PREVIEW_EXIT_DIAMETER_VP &&
    PREVIEW_EXIT_DIAMETER_VP > 0,
    '两档门槛都是"进入值 > 保持值 > 0"的滞回对，不是机型表')
  const ctx = (w, h) => ({ viewportWidthVp: w, viewportHeightVp: h, focusScenePath: 'root' })
  const detailAtShortSide = (shortSide, scale) =>
    detailOf('root/embed:a', scale, ctx(shortSide, shortSide))
  // coverage = 200 × scale / 短边，是纯连续量
  assert(near(projectEmbedMetrics(1, ctx(360, 800)).coverage, 200 / 360) &&
    near(projectEmbedMetrics(1, ctx(1200, 2400)).coverage, 200 / 1200),
    'coverage 恒等于 圆投影 / 视口短边（同一个公式）')
  assert(detailAtShortSide(360, 1) === 'preview' && detailAtShortSide(360, 2) === 'interactive',
    '360×640 小窗：相机 1 → preview，相机 2（coverage 1.11）→ interactive')
  assert(detailAtShortSide(200, 1) === 'interactive',
    '短边 200 时 200vp 的圆正好占满短边 → interactive（不是"按层数限深"）')
  assert(detailAtShortSide(2000, 1) === 'preview',
    '短边 2000 时 200vp 的圆只给 preview：投影直径还是 200vp，缩略图看得清，组件装不下')
  assert(detailAtShortSide(2000, 0.1) === 'shell',
    '同一条 2000vp 圆缩到 0.1 → 直径 20vp，只剩圆壳（连续判据，不是按层数保底）')
  const order = { shell: 0, preview: 1, interactive: 2 }
  let prev = Infinity
  let monotone = true
  for (let shortSide = 200; shortSide <= 1600; shortSide += 20) {
    const current = order[detailAtShortSide(shortSide, 2.5)]
    if (current > prev) { monotone = false }
    prev = current
  }
  assert(monotone, '视口越大档位只降不升（连续，没有台阶）')
  const unmeasured = projectEmbedMetrics(1, ctx(0, 0))
  assert(unmeasured.coverage === 0 && !Number.isNaN(unmeasured.coverage) &&
    detailOf('root/embed:a', 1, ctx(0, 0)) === 'preview',
    '视口还没量出来时 coverage 记 0（不是 NaN），档位只由投影直径兜底')
}

console.log('38b. 真实递归链路：child local fit → 下一层累计比例 → 下一层档位')
{
  // 不再手填 ownerEffectiveScale：按真实链路一级一级算下去。
  // 每层 child Scene 的局部盒子恒为 DEFAULT_EMBED_DIAMETER，
  // 内容随层数变宽（越深的子星图内容越多），所以累计 fit 会自然把更深层压下去。
  const contentBoundsAtDepth = (depth) => ({ minX: 0, minY: 0, maxX: 240 * depth, maxY: 120 * depth, width: 240 * depth, height: 120 * depth })
  const walkChain = (context, depthCount) => {
    const details = []
    const fits = []
    let ownerEffectiveScale = 1 // 根 Scene：camera = 1，没有祖先 local fit
    for (let depth = 1; depth <= depthCount; depth++) {
      const embedScenePath = depth === 1
        ? 'root/embed:a'
        : 'root/embed:a' + '/embed:b'.repeat(depth - 1)
      const detail = detailOf(embedScenePath, ownerEffectiveScale, context)
      details.push(detail)
      if (detail === 'shell') { break }
      // child Scene：局部盒子恒为默认直径，可用区按圆内接正方形算
      const childBox = DEFAULT_EMBED_DIAMETER
      const childAvailable = computeEmbedInnerContentSafeSide(childBox, EMBED_FIT_PADDING_VP)
      const fitted = computeFittedViewport(contentBoundsAtDepth(depth), childAvailable, childAvailable, 0, childBox, childBox)
      fits.push(fitted.zoomScale)
      ownerEffectiveScale = ownerEffectiveScale * fitted.zoomScale
    }
    return { details, fits }
  }

  const small = { viewportWidthVp: 360, viewportHeightVp: 640, focusScenePath: 'root' }
  const large = { viewportWidthVp: 1000, viewportHeightVp: 900, focusScenePath: 'root' }
  const smallChain = walkChain(small, 6)
  const largeChain = walkChain(large, 6)
  console.log(`     小窗链: ${smallChain.details.join(' → ')} | 大窗链: ${largeChain.details.join(' → ')}`)

  // 相机 1 时第一层都是 preview：要占满视口短边 70% 才挂完整交互组件。
  assert(smallChain.details[0] === 'preview' && largeChain.details[0] === 'preview',
    '相机 1 时第一层只是 preview（不是 expanded / collapsed 二分）')
  assert(smallChain.details.includes('shell') && largeChain.details.includes('shell'),
    '相机 1 时再深的层自动掉到 shell —— "两层 / 三层"只是某个视口下的观测结果')

  // 相机放到能占满短边时，第一层真的进 interactive，且交互层数随祖先 local fit 变多
  const interactiveDepth = (cameraScale, localFitPerLevel) => {
    let scale = cameraScale
    let count = 0
    let previous = null
    for (let depth = 0; depth < 12; depth++) {
      const detail = detailOf('root/embed:a', scale, large, previous)
      if (detail !== 'interactive') { break }
      previous = detail
      count += 1
      scale *= localFitPerLevel
    }
    return count
  }
  assert(interactiveDepth(3.2, 0.45) > 0 && interactiveDepth(3.2, 0.9) > interactiveDepth(3.2, 0.45),
    '祖先 local fit 越松 → 能交互的层数越多（同一份公式，没有深度常量）')
  assert(interactiveDepth(1, 0.45) === 0 && interactiveDepth(4, 0.45) > interactiveDepth(1.4, 0.45),
    '相机放大 → 能交互的层数变多')

  // 深层的档位完全由祖先 local fit 的连乘决定
  assert(largeChain.fits.length > 0 && largeChain.fits.every(f => f < 1),
    '每层 local fit 都 < 1：越深的 Embed 投影越小（没有任何"越深越大"的机制）')
  assert(near(largeChain.fits[0], 0.371, 0.01),
    `第一层 local fit = ${largeChain.fits[0].toFixed(3)}（按默认直径的圆内接正方形可用区现算，与视口无关）`)
  assert(near(smallChain.fits[0], largeChain.fits[0]),
    '两层链的 local fit 完全一样：视口大小不改变子图的局部 fit（只改档位）')
}

console.log('38b-2. 局部 box 恒为默认直径：resize / zoom 都不重算 fit（#821）')
{
  const childBounds = { minX: 0, minY: 0, maxX: 240, maxY: 120, width: 240, height: 120 }
  const available = computeEmbedInnerContentSafeSide(DEFAULT_EMBED_DIAMETER, EMBED_FIT_PADDING_VP)
  const fitted = computeFittedViewport(childBounds, available, available, 0,
    DEFAULT_EMBED_DIAMETER, DEFAULT_EMBED_DIAMETER)
  assert(near(fitted.zoomScale, available / childBounds.width) &&
    fitted.zoomScale < available / DEFAULT_EMBED_DIAMETER,
    `child fit = ${fitted.zoomScale.toFixed(3)}：可用区只由默认直径算出（与视口、相机都无关），再被内容宽高收一次`)

  // 同理，"按某个更大的盒子 fit"这件事本身已经不存在了
  const biggerBox = 320
  const availableInBigger = computeEmbedInnerContentSafeSide(biggerBox, EMBED_FIT_PADDING_VP)
  const fittedInBigger = computeFittedViewport(childBounds, availableInBigger, availableInBigger, 0, biggerBox, biggerBox)
  assert(fittedInBigger.zoomScale > fitted.zoomScale,
    '（反事实）如果盒子真变成 320，fit 会更大 —— 但 #821 之后盒子永远不变，这条路走不到')

  const sceneSource821 = readStarmapSource('ui/StarMapScene.ets')
  assert(!sceneSource821.includes('syncFitToSceneSize'),
    '没有"尺寸变了要重算 fit"的路径：局部 box 恒为 DEFAULT_EMBED_DIAMETER')
  assert(!sceneSource821.includes('lastFittedSceneSize'),
    '没有"上次 fit 时用的尺寸"这个状态')
  assert(sceneSource821.includes('private localSceneSize(): number {') &&
    /private localSceneSize\(\): number \{\s*return this\.sceneWidth/.test(sceneSource821),
    'localSceneSize 不再除以 parentScale（box 就是本地尺寸）')
  const fitBody821 = sceneSource821.slice(sceneSource821.indexOf('private applyFittedLocalViewport('),
    sceneSource821.indexOf('private syncSceneHandle()'))
  assert(fitBody821.includes('computeEmbedInnerContentSafeSide(') && fitBody821.includes('computeFittedViewport('),
    'fit 走圆内接正方形可用区（与 clamp / preview 缩略图同一口径）')
  assert(!fitBody821.includes('computeCenteredOffset'),
    'fit 不再"只 recenter"：比例和偏移一起算')
  const firstFitIdx821 = sceneSource821.indexOf('fitView(): void {')
  assert(sceneSource821.slice(firstFitIdx821, firstFitIdx821 + 260).includes('applyFittedLocalViewport('),
    '首次 fit 也走同一个出口，两条路径不会长歪')
  const transformBody821 = sceneSource821.slice(sceneSource821.indexOf('private onTransformChange()'),
    sceneSource821.indexOf('private syncSceneHandle()'))
  assert(!transformBody821.includes('applyFittedLocalViewport') &&
    !transformBody821.includes('buildEmbedLayoutNodes'),
    '相机变化只刷新显示变换，不重算 fit、不重建布局')
}

console.log('38c. 视口 resize 不改焦点；焦点只跟着相机走')
{
  const screenSource821 = readStarmapSource('ui/StarMapScreen.ets')
  const areaBody = screenSource821.slice(screenSource821.indexOf('.onAreaChange('),
    screenSource821.indexOf('.onAreaChange(') + 900)
  assert(!areaBody.includes('recomputeVisualFocus'),
    '纯 onAreaChange 只更新视口尺寸，不重算视觉焦点（折叠屏展开一下不该换焦点）')
  assert(screenSource821.includes('this.canvasWidth = Number(newArea.width)') &&
    screenSource821.includes('this.canvasHeight = Number(newArea.height)'),
    'onAreaChange 更新的是根视口宽高')
  assert(screenSource821.includes('projectEmbedMetrics(probe.ownerScale, context)'),
    '焦点覆盖率直接用稳定圆投影（没有"视口感知的展开尺寸"这一层）')
  assert(!screenSource821.includes('expandedEmbedSizeVp') && !screenSource821.includes('ZOOM_STEP'),
    'Screen 里不再有展开尺寸，也没有加性 ZOOM_STEP')
  assert(screenSource821.includes('const ZOOM_FACTOR: number = 1.2') &&
    screenSource821.includes('this.cameraScale * ZOOM_FACTOR') &&
    screenSource821.includes('this.cameraScale / ZOOM_FACTOR'),
    '工具栏缩放改成乘性 ZOOM_FACTOR（无级缩放回到手感上）')

  const sceneSource821 = readStarmapSource('ui/StarMapScene.ets')
  const detailBody821 = sceneSource821.slice(sceneSource821.indexOf('private embedDetail('),
    sceneSource821.indexOf('private embedDetail(') + 800)
  assert(detailBody821.includes('this.viewportScaleValue()') && detailBody821.includes('this.visualLodContext()'),
    'embedDetail 用所属 Scene 的累计比例 + 根视口上下文算档位')
  assert(sceneSource821.includes('updateStarMapEmbed(this.starmapId, embedId,') &&
    !sceneSource821.includes('embedAuthoredPositionFromDisplay'),
    '移动保存直接写 authored 坐标（显示矩形不再需要反解）')
  const layoutSource821 = readStarmapSource('platform/StarMapLayout.ets')
  assert(layoutSource821.includes('x: embed.position.x') && layoutSource821.includes('y: embed.position.y') &&
    !layoutSource821.includes('authoredCenterX'),
    '布局矩形的位置就是 authored position（不再以中心为锚派生）')
}

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
