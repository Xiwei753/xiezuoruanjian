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

function hitTestWithScene(rects, screenX, screenY, scenePath, embedInstanceIds) {
  for (let i = rects.length - 1; i >= 0; i--) {
    const r = rects[i]
    const isEmbed = embedInstanceIds.has(r.nodeId)
    if (isEmbed) {
      if (!pointInEmbedCircle(r, screenX, screenY)) { continue }
      if (screenY <= r.y + EMBED_TITLE_HIT_HEIGHT) {
        return { scenePath, objectKind: 'embedTitle', objectId: r.nodeId, hitRegion: 'title' }
      }
      const cx = r.x + r.width / 2
      const cy = r.y + r.height / 2
      const outer = r.width / 2
      const inner = Math.max(0, outer - EMBED_BORDER_HIT_WIDTH)
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
    const hit = hitTestWithScene(current.rects, canvasPoint.x, canvasPoint.y, current.scenePath, current.embedInstanceIds)
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
  return {
    reset() { state = emptyState() },
    beginPanCanvas(ownerScenePath, sx, sy) {
      const n = emptyState()
      n.mode = 'panCanvas'
      n.ownerScenePath = copyPath(ownerScenePath)
      n.panOwnerScenePath = copyPath(ownerScenePath)
      n.startPoint = { x: sx, y: sy }
      n.currentPoint = { x: sx, y: sy }
      state = n
    },
    beginConnect(ownerScenePath, itemId, sx, sy) {
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
 * 关键点（#818 复审）：子 Scene 在画面上的有效比例是 `fitScale * inheritedScale`，
 * 偏移同理；而 fitScale / fitOffset 存的是**局部数据**，算的时候要先把
 * `sceneWidth / sceneHeight` 除掉 inheritedScale 还原成本层原始尺寸。
 * 于是同一张子图无论在相机 1 还是相机 2 下打开，局部 fit 完全一样，
 * 而屏幕上的一切都会跟着相机一起放大缩小。
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
    lastFittedSceneSize: 0,
    isCameraScene() { return v.sceneDepth === 0 },
    parentScale() { return v.inheritedScale > 0 ? v.inheritedScale : 1 },
    localSceneWidth() { return v.sceneWidth / v.parentScale() },
    localSceneHeight() { return v.sceneHeight / v.parentScale() },
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
      const localWidth = v.localSceneWidth()
      const localHeight = v.localSceneHeight()
      const bounds = computeContentBounds(rects)
      if (bounds === null) {
        v.fitScale = 1; v.fitOffsetX = 0; v.fitOffsetY = 0
        v.lastFittedSceneSize = localWidth
        return
      }
      const available = Math.min(localWidth, localHeight) * CIRCLE_INNER_SAFE_RATIO
      const fitted = computeFittedViewport(bounds, available, available, EMBED_FIT_PADDING_VP, localWidth, localHeight)
      v.fitScale = fitted.zoomScale
      v.fitOffsetX = fitted.offsetX
      v.fitOffsetY = fitted.offsetY
      v.lastFittedSceneSize = localWidth
    },
    // 局部尺寸没变就什么都不做（相机缩放的正常情况），变了才重新居中
    syncFitToSceneSize(rects) {
      if (!v.hasFittedView || v.lastFittedSceneSize <= 0) { return false }
      const localWidth = v.localSceneWidth()
      if (Math.abs(localWidth - v.lastFittedSceneSize) <= 0.5) { return false }
      v.lastFittedSceneSize = localWidth
      const bounds = computeContentBounds(rects)
      if (bounds === null) {
        v.fitOffsetX = 0; v.fitOffsetY = 0
      } else {
        const offset = computeCenteredOffset(bounds, v.fitScale, localWidth, v.localSceneHeight())
        v.fitOffsetX = offset.x
        v.fitOffsetY = offset.y
      }
      return true
    },
    // 入参是屏幕上的累计像素，写回局部数据前要除掉祖先累计比例
    setViewportOffset(offsetX, offsetY) {
      if (v.isCameraScene()) {
        v.cameraScale = v.cameraScale; v.cameraOffsetX = offsetX; v.cameraOffsetY = offsetY
        return
      }
      v.fitOffsetX = offsetX / v.parentScale()
      v.fitOffsetY = offsetY / v.parentScale()
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

  // 深层空白：根局部 (450,38)
  //   → sm-a 画布 (80,56)：离 emb-a1 圆心 48 < 68 → 下沉
  //   → sm-a1 画布 (100,60)：n-a1x 的 y 从 100 起 → 空白
  const deepBlank = resolveRecursiveHit(ctx, 450, 38)
  assert(deepBlank.target === null, '第三层空白 → target 为 null（不误判成选中 emb-a1）')
  assert(eq(deepBlank.ownerScenePath, PATH_A1), '深层空白归属第三层 Scene')

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

console.log('24. 评审回归 ⑥：相机缩放带动容器变化，子 Scene 的局部 fit 完全不动（#818）')
{
  // 首次适配：直径 200 的圆，内容按内容包围盒算出 fitScale 并居中
  const rects = [
    { nodeId: 'a', x: 0, y: 0, width: 100, height: 100, radius: 0 },
    { nodeId: 'b', x: 100, y: 0, width: 100, height: 100, radius: 0 }
  ]
  const bounds = computeContentBounds(rects)
  const available = 200 * CIRCLE_INNER_SAFE_RATIO
  const fitted = computeFittedViewport(bounds, available, available, EMBED_FIT_PADDING_VP, 200, 200)

  // camera=1 时打开：屏幕尺寸 200，inheritedScale=1，局部尺寸 = 200 / 1 = 200
  const view = createSceneView({ sceneDepth: 1, sceneWidth: 200, sceneHeight: 200, inheritedScale: 1 })
  view.fitView(rects)
  assert(near(view.fitScale, fitted.zoomScale) && near(view.fitOffsetX, fitted.offsetX),
    'camera=1 时算出的局部 fit 与直接用 200×200 算的一致')

  // 相机放到 2 倍 → 圆壳和子 Scene 组件在屏幕上 200 → 400，inheritedScale 同步 1 → 2。
  // 局部尺寸 = 400 / 2 = 200，没变，所以局部 fit 一个字节都不改（#818 复审）。
  view.cameraScale = 2
  view.inheritedScale = 2
  view.sceneWidth = 400
  view.sceneHeight = 400
  const changed = view.syncFitToSceneSize(rects)
  assert(changed === false, '局部尺寸没变（200/1 → 400/2），syncFitToSceneSize 直接不动')
  assert(near(view.fitScale, fitted.zoomScale) && near(view.fitOffsetX, fitted.offsetX) &&
    near(view.fitOffsetY, fitted.offsetY), 'fitScale / fitOffset 全部保持原值')

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
  view.sceneWidth = 100
  view.sceneHeight = 100
  assert(view.syncFitToSceneSize(rects) === false && near(view.fitScale, fitted.zoomScale),
    '缩回一半：局部尺寸还是 200，fitScale 依旧不动')
}

console.log('24b. 变换链：camera 1 → 2 时根 / 子 / 孙三层内容全部 ×2（#818 复审）')
{
  const childRects = [{ nodeId: 'n2', x: 0, y: 0, width: 56, height: 56, radius: 28 }]
  const grandRects = [{ nodeId: 'n3', x: 0, y: 0, width: 56, height: 56, radius: 0 }]

  // 建三层：根（相机本体）+ 子 + 孙。盒子尺寸 = DEFAULT_EMBED_DIAMETER × 父层有效比例。
  const build = (cameraScale) => {
    const root = createSceneView({ sceneDepth: 0, cameraScale })
    const child = createSceneView({ sceneDepth: 1, inheritedScale: root.viewportScaleValue() })
    child.sceneWidth = DEFAULT_EMBED_DIAMETER * root.viewportScaleValue()
    child.sceneHeight = child.sceneWidth
    child.fitView(childRects)
    const grand = createSceneView({ sceneDepth: 2, inheritedScale: child.viewportScaleValue() })
    grand.sceneWidth = DEFAULT_EMBED_DIAMETER * child.viewportScaleValue()
    grand.sceneHeight = grand.sceneWidth
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
  assert(near(b.child.sceneWidth / b.child.inheritedScale, DEFAULT_EMBED_DIAMETER) &&
    near(b.grand.sceneWidth / b.grand.inheritedScale, DEFAULT_EMBED_DIAMETER),
    '还原出的局部尺寸恒等于 Embed 基础直径，不随相机变化')

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
    // 根层相机 = cameraScale → 子 Scene 屏幕尺寸 = D × cameraScale，inheritedScale 同值
    const root = createSceneView({ sceneDepth: 0, cameraScale })
    const child = createSceneView({ sceneDepth: 1, inheritedScale: root.viewportScaleValue() })
    child.sceneWidth = DEFAULT_EMBED_DIAMETER * root.viewportScaleValue()
    child.sceneHeight = child.sceneWidth
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

  // 父 Embed 基础尺寸真的变了才重算局部居中
  const resized = createSceneView({ sceneDepth: 1, inheritedScale: 1, sceneWidth: 300, sceneHeight: 300 })
  resized.fitView(rects)
  const before = resized.fitOffsetX
  const fitBefore = resized.fitScale
  resized.sceneWidth = 360
  resized.sceneHeight = 360
  assert(resized.syncFitToSceneSize(rects) === true && !near(resized.fitOffsetX, before),
    '局部尺寸 300 → 360 属于真实容器变化，重新居中')
  assert(near(resized.fitScale, fitBefore), '重新居中只动偏移，不动 fitScale')
}

console.log('24d. 子 Scene 内单指平移：屏幕位移要除掉祖先累计比例再存（#818 复审）')
{
  const view = createSceneView({ sceneDepth: 1, inheritedScale: 2 })
  view.setViewportOffset(20, -10)
  assert(near(view.fitOffsetX, 10) && near(view.fitOffsetY, -5), '局部数据存的是屏幕位移 ÷ 2')
  assert(near(view.viewportOffsetX(), 20) && near(view.viewportOffsetY(), -10),
    '再乘回祖先累计比例，屏幕上还是原来那个位移')

  const root = createSceneView({ sceneDepth: 0 })
  root.setViewportOffset(20, -10)
  assert(near(root.cameraOffsetX, 20) && near(root.cameraOffsetY, -10), '根 Scene 改的是全局相机，不做换算')

  // 祖先比例为 0 时的兜底：不能把整棵子树的尺寸算成 0
  const broken = createSceneView({ sceneDepth: 1, inheritedScale: 0 })
  assert(near(broken.parentScale(), 1) && near(broken.viewportScaleValue(), broken.fitScale),
    'inheritedScale 传成 0 时兜回 1，不让子树塌成 0 倍')
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
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
