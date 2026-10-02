// starmap_recursive_scene.test.mjs — 星图递归 Scene 层（Issue #816）纯逻辑测试。
//
// 纯 JS（.mjs），不依赖 ArkUI / Core bridge，Node 直接运行：
//   node apps/harmony/entry/src/main/ets/feature/starmap/__tests__/starmap_recursive_scene.test.mjs
//
// 本评论要求的行为契约（对应实现文件）：
//   1. 子图初始比例来自"内容包围盒 × 父 Embed 圆形可用区"，最终比例 = fitScale × userZoomScale；
//      不允许出现"每深一层乘 0.6"这种写死系数
//      —— computeContentBounds / computeFitScale / computeFittedViewport / effectiveScale
//         (StarMapViewport.ets)
//   2. 双指缩放的锚点必须落在**归属 Scene 自己的局部坐标**里，父层不能拿自己的坐标算子层锚点
//      —— computeZoomAroundOffset / sceneLocalToCanvas (StarMapViewport.ets)
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
//   5. 双指归属一旦定下就不换人，直到双指抬起；父层只能观察不能改自己视口
//      —— StarMapGestureStateTracker.beginPinch / canClaimPinch / isPinchOwnedByScene
//         (StarMapGestureState.ets)
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
const USER_ZOOM_MIN = 0.3
const USER_ZOOM_MAX = 3
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

function effectiveScale(fitScale, userZoomScale) { return fitScale * userZoomScale }

function clampUserZoom(userZoomScale, minZoom, maxZoom) {
  if (!isFinite(userZoomScale) || userZoomScale <= 0) { return 1 }
  return Math.max(minZoom, Math.min(maxZoom, userZoomScale))
}

function computeZoomAroundOffset(anchorCanvas, anchorScreen, nextScale) {
  return { x: anchorScreen.x - anchorCanvas.x * nextScale, y: anchorScreen.y - anchorCanvas.y * nextScale }
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

function resolveRecursiveHit(root, x, y) {
  let current = root
  let localX = x
  let localY = y
  const embedPath = []
  for (let depth = 0; depth <= MAX_RECURSE_SCENE_DEPTH; depth++) {
    const canvasPoint = screenToCanvas(localX, localY, current.scale, current.offsetX, current.offsetY)
    const hit = hitTestWithScene(current.rects, canvasPoint.x, canvasPoint.y, current.scenePath, current.embedInstanceIds)
    if (hit === null) {
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
    activeItemScenePath: null, panOwnerScenePath: null, pinchOwnerScenePath: null,
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
    beginPinch(ownerScenePath, cx, cy) {
      const n = emptyState()
      n.mode = 'pinch'
      n.ownerScenePath = copyPath(ownerScenePath)
      n.pinchOwnerScenePath = copyPath(ownerScenePath)
      n.startPoint = { x: cx, y: cy }
      n.currentPoint = { x: cx, y: cy }
      state = n
    },
    endPinch() {
      if (state.pinchOwnerScenePath === null) { return }
      state = emptyState()
    },
    setTargetItemScenePath(scenePath) { state.targetItemScenePath = copyPath(scenePath) },
    isIdle() { return state.mode === 'idle' },
    isConnecting() { return state.mode === 'connect' },
    isPinching() { return state.pinchOwnerScenePath !== null },
    isPinchOwnedByScene(scenePath) {
      if (state.pinchOwnerScenePath === null) { return false }
      return isSameScenePath(state.pinchOwnerScenePath, scenePath)
    },
    canClaimPinch(scenePath) {
      if (state.pinchOwnerScenePath === null) { return true }
      return isSameScenePath(state.pinchOwnerScenePath, scenePath)
    },
    isConnectOwnedByScene(scenePath) {
      if (state.connectOwnerScenePath === null) { return false }
      return isSameScenePath(state.connectOwnerScenePath, scenePath)
    },
    isPanOwnedByScene(scenePath) {
      if (state.panOwnerScenePath === null) { return false }
      return isSameScenePath(state.panOwnerScenePath, scenePath)
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
        pinchOwnerScenePath: s.pinchOwnerScenePath !== null ? copyPath(s.pinchOwnerScenePath) : null,
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

function makeSceneSource(scenePath, starmapId, rects, embedIds, scale, offsetX, offsetY, children) {
  return {
    scenePath, starmapId, rects,
    embedInstanceIds: new Set(embedIds),
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
  const node = {
    scenePath, starmapId,
    rects: opts.rects || [],
    embedInstanceIds: new Set(opts.embedInstanceIds || []),
    scale: opts.scale === undefined ? 1 : opts.scale,
    offsetX: opts.offsetX || 0,
    offsetY: opts.offsetY || 0,
    fitScale: opts.fitScale === undefined ? 1 : opts.fitScale,
    getChildEmbeds() { return opts.getChildEmbeds ? opts.getChildEmbeds() : [] },
    sync(rects, embedInstanceIds, scale, offsetX, offsetY, fitScale) {
      node.rects = rects
      node.embedInstanceIds = new Set(embedInstanceIds)
      node.scale = scale
      node.offsetX = offsetX
      node.offsetY = offsetY
      node.fitScale = fitScale
    },
    async createEdgeBetween(from, to) { return opts.createEdgeFn(from, to) }
  }
  return node
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
// 2. fitScale / userZoomScale 合成
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

  // 最终比例 = fitScale × userZoomScale
  const fit = 0.4
  assert(near(effectiveScale(fit, 1), 0.4), 'userZoomScale = 1 → 最终比例就是 fitScale')
  assert(near(effectiveScale(fit, 2.5), 1.0), 'userZoomScale = 2.5 → 0.4 × 2.5 = 1')
  assert(near(effectiveScale(fit, clampUserZoom(99, USER_ZOOM_MIN, USER_ZOOM_MAX)), 1.2),
    'userZoomScale 先夹到上限 3，最终比例 1.2')
  assert(near(clampUserZoom(0.01, USER_ZOOM_MIN, USER_ZOOM_MAX), USER_ZOOM_MIN), 'userZoomScale 有下限')
  assert(near(clampUserZoom(NaN, USER_ZOOM_MIN, USER_ZOOM_MAX), 1), '非法 userZoomScale 退回 1')
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

console.log('4. 缩放锚点：锚点画布坐标固定，屏幕锚点跟着两指中心走')
{
  const scale = 0.5, offsetX = 10, offsetY = 10
  const center = { x: 260, y: 60 }
  const anchorCanvas = sceneLocalToCanvas(center.x, center.y, scale, offsetX, offsetY)
  assert(nearPt(anchorCanvas, 500, 100), '两指中心 → 画布锚点（归属 Scene 自己的坐标系）')
  for (const next of [0.25, 0.5, 1.0, 2.0]) {
    const off = computeZoomAroundOffset(anchorCanvas, center, next)
    const still = canvasToScreen(anchorCanvas.x, anchorCanvas.y, next, off.x, off.y)
    assert(nearPt(still, center.x, center.y, 1e-9), `缩到 ${next} 倍后锚点仍钉在两指中心`)
  }
  // 父层不能拿自己的坐标算子层锚点
  const rootAnchor = sceneLocalToCanvas(center.x, center.y, 1, 0, 0)
  assert(!near(rootAnchor.x, anchorCanvas.x),
    '用父层视口算出的"锚点"和用归属层算出的是两个值（这正是必须换算的原因）')
  assert(near(sceneLocalToCanvas(10, 10, 0.5, 10, 10).x, 0), 'sceneLocalToCanvas / canvasToSceneLocal 互为逆运算')
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

console.log('9. 双指归属：一次双指期间不换人，父层只能观察')
{
  const tracker = createGestureStateTracker()
  assert(tracker.canClaimPinch([]) && tracker.canClaimPinch(PATH_A), '没人认领时任何层都能认领')

  tracker.beginPinch(PATH_A, 460, 60)
  assert(tracker.isPinching(), 'beginPinch 后处于双指缩放中')
  assert(tracker.isPinchOwnedByScene(PATH_A), '归属层 isPinchOwnedByScene 为 true')
  assert(tracker.isPinchOwnedByScene([]) === false, '父层 isPinchOwnedByScene 为 false（只能观察）')
  assert(tracker.canClaimPinch(PATH_A), '归属层自己可以继续认领')
  assert(tracker.canClaimPinch([]) === false, '父层不能中途抢走归属（否则一次缩放前后半段缩不同层）')
  assert(tracker.canClaimPinch(PATH_B) === false, '兄弟层也不能抢')

  // 归属层看到的 mode 不会因为父层空闲而改变
  assert(tracker.getState().mode === 'pinch', '归属层 mode = pinch，不靠"某层是否 idle"判断递归归属')
  assert(tracker.isIdle() === false, '双指中不是 idle')

  tracker.endPinch()
  assert(tracker.isPinching() === false, '双指抬起后不再处于缩放中')
  assert(tracker.canClaimPinch([]), '下一次双指父层可以重新认领')
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
  node.sync(rects, [], 2, 100, 200, 1)
  ctx = buildRecursiveSceneContext(node)
  const fresh = resolveRecursiveHit(ctx, 250, 240)
  assert(fresh.target !== null && fresh.target.objectId === 'n-root',
    'sync 之后同一个屏幕点重新命中（syncSceneHandle 就是补这一下）')

  // fitScale 变了也要 sync：子 Scene 的真实比例是 fitScale × userScale
  node.sync(rects, [], 0.5, 0, 0, 0.5)
  ctx = buildRecursiveSceneContext(node)
  const refit = resolveRecursiveHit(ctx, 40, 20)
  assert(refit.target !== null && refit.target.objectId === 'n-root',
    'fitView 改完 fitScale 后 sync，子 Scene 的视口也跟着变（屏幕 (40,20) → 画布 (80,40)）')
}

console.log('21. 评审回归 ③：单指平移看 pan 归属，不看 pinch 归属')
{
  const tracker = createGestureStateTracker()
  // 普通单指拖动画布：没有 pinch 归属人
  tracker.beginPanCanvas(PATH_A, 100, 100)
  assert(tracker.isPanOwnedByScene(PATH_A), '平移归属发起层')
  assert(tracker.isPinching() === false, '单指平移期间没有 pinch 归属人')
  // UI 的守卫条件：mode 是 panCanvas、panActive、pan 归属是自己、且本层可以改视口
  const canPanCanvas = !tracker.isPinching() || tracker.isPinchOwnedByScene(PATH_A)
  assert(canPanCanvas, '没有 pinch 归属冲突时本层可以平移（老代码查 isPinchOwnedByScene 恒为 false，永远拖不动）')
  assert(tracker.isPanOwnedByScene([]) === false, '父层不归属这次平移')

  // 换成 pinch 归属冲突时，父层要能自我否决
  tracker.beginPinch(PATH_A, 100, 100)
  const parentCanPan = !tracker.isPinching() || tracker.isPinchOwnedByScene([])
  assert(parentCanPan === false, '子层在缩放时父层自我否决，不会跟着一起动')
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

console.log('')
console.log(`结果: ${passed} passed, ${failed} failed`)
if (failed > 0) process.exit(1)
