// ─── StarMapScene.ets ───
// 可递归星图场景组件：负责某一张星图的完整显示和交互。
// 可嵌套在 StarMapScreen（根场景）或 StarMapEmbedScene（子星图 Embed 内部）中。
//
// 关键约束：
// 1. 所有 gestureTracker.begin* 调用传入 this.scenePath 作为 ownerScenePath
// 2. 所有 bridge mutation 调用使用 this.starmapId（当前 scene 的 starmapId）
// 3. Embed 的 title/border 区域负责选中 Embed；innerContent 区域让事件穿透到 child StarMapScene
// 4. 递归结构：StarMapScene → StarMapEmbedScene → child StarMapScene

import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import {
  StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed,
  StarMapTargetPath, StarMapPoint, StarMapPathSegment
} from '../../../corebridge/dto/StarMapDtos'
import { ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import {
  PlatformNodeRect, PlatformEdgeRender,
  deriveNodeRectFromLayoutNode,
  hitTestWithScene, computeAllEdgeRenders,
  screenToCanvas, resolveEdgeEndpointAnchor
} from '../platform/StarMapGeometry'
import {
  PlatformLayoutNode, PlatformPoint,
  buildFreeformLayout, buildEmbedLayoutNodes, applyFreeformLayout
} from '../platform/StarMapLayout'
import {
  PlatformMotionConfig,
  wobbleXFromProgress, wobbleYFromProgress,
  MotionPauseTracker
} from '../platform/StarMapMotion'
import { StarMapGestureMode, StarMapGestureStateTracker } from '../platform/StarMapGestureState'
import { StarMapEmbedScene } from './StarMapEmbedScene'

/** wobble 视觉偏移（dx, dy）：只影响显示，不写回 Core。 */
interface WobbleOffset {
  dx: number
  dy: number
}

@Component
export struct StarMapScene {
  // ─── @Prop：从父组件只读传入 ───
  /** 根星图 ID（整个 StarMapScreen 生命周期内不变） */
  @Prop rootStarmapId: string = ''
  /** 当前 scene 的路径段（根 scene 为空数组） */
  @Prop scenePath: StarMapPathSegment[] = []
  /** 当前 scene 对应的星图 ID */
  @Prop starmapId: string = ''
  /** 当前 scene 的深度（根为 0） */
  @Prop sceneDepth: number = 0
  /** 主题配置 */
  @Prop themeConfig: ThemeConfig = {
    mode: 'light',
    colorSource: 'built_in',
    selectedPaletteId: '',
    selectedBuiltinThemeId: '',
    colors: {
      primary: '#000000', onPrimary: '#FFFFFF',
      primaryContainer: '#E0E0E0', onPrimaryContainer: '#1A1C1E',
      secondary: '#51606F', onSecondary: '#FFFFFF',
      secondaryContainer: '#D5E2F0', onSecondaryContainer: '#0D1E2C',
      tertiary: '#695779', onTertiary: '#FFFFFF',
      tertiaryContainer: '#F0DBFF', onTertiaryContainer: '#241533',
      error: '#BA1A1A', onError: '#FFFFFF',
      errorContainer: '#FFDAD6', onErrorContainer: '#410002',
      background: '#FDFCFF', onBackground: '#1A1C1E',
      surface: '#FDFCFF', onSurface: '#1A1C1E',
      surfaceVariant: '#DFE2EB', onSurfaceVariant: '#43474E',
      surfaceContainerLowest: '#FFFFFF', surfaceContainerLow: '#F7F8FA',
      surfaceContainer: '#F1F2F5', surfaceContainerHigh: '#EBEDF0',
      surfaceContainerHighest: '#E6E7EA', surfaceDim: '#DAD9DD',
      surfaceBright: '#FDFCFF',
      outline: '#73777F', outlineVariant: '#C3C7CF',
      inverseSurface: '#2F3033', inverseOnSurface: '#F1F0F4',
      inversePrimary: '#A8C8FF',
      scrim: '#000000',
      textPrimary: '#1A1C1E', textSecondary: '#43474E',
      textHint: '#73777F', success: '#2ECC71',
      divider: '#E0E0E0', overlay: '#80000000'
    },
    fontSize: 14,
    fontFamily: 'HarmonyOS Sans',
    lineHeight: 1.5
  }
  /** 运动配置 */
  @Prop motionConfig: PlatformMotionConfig = {
    enabled: true, reduceMotion: false,
    idleWobbleEnabled: true, idleAmplitudeVp: 2.0,
    idlePeriodMs: 4200, dragLiftScale: 1.04,
    settleDurationMs: 220
  }
  /** 动画进度 */
  @Prop @Watch('onMotionProgressChange') motionProgress: number = 0

  // ─── @Link：与父组件双向绑定 ───
  /** 当前星图的图数据 */
  @Link @Watch('onGraphChange') graph: StarMapGraph | null
  /** 当前 scene 的缩放 */
  @Link @Watch('onTransformChange') zoomScale: number
  /** 当前 scene 的 X 偏移 */
  @Link @Watch('onTransformChange') offsetX: number
  /** 当前 scene 的 Y 偏移 */
  @Link @Watch('onTransformChange') offsetY: number
  /** 当前选中对象 ID */
  @Link @Watch('onTransformChange') selectedNodeId: string

  // ─── 普通属性：共享对象和回调 ───
  /** Core bridge（用于 mutation） */
  bridge: IWriterCoreBridge | null = null
  /** 手势状态追踪器（共享） */
  gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  /** 选中回调 */
  onSelectItem: (scenePath: StarMapPathSegment[], itemId: string) => void = (_path, _id) => {}
  /** 新建节点回调 */
  onCreateNode: (scenePath: StarMapPathSegment[], starmapId: string, title: string, position: StarMapPoint) => void = (_path, _id, _title, _pos) => {}
  /** 新建子星图回调 */
  onCreateEmbed: (scenePath: StarMapPathSegment[], starmapId: string, title: string, position: StarMapPoint) => void = (_path, _id, _title, _pos) => {}
  /** 新建边回调 */
  onCreateEdge: (scenePath: StarMapPathSegment[], starmapId: string, fromItemId: string, toItemId: string) => void = (_path, _id, _from, _to) => {}
  /** 移动节点回调 */
  onMoveNode: (scenePath: StarMapPathSegment[], starmapId: string, nodeId: string, position: StarMapPoint) => void = (_path, _id, _nodeId, _pos) => {}
  /** 移动 Embed 回调 */
  onMoveEmbed: (scenePath: StarMapPathSegment[], starmapId: string, embedInstanceId: string, position: StarMapPoint) => void = (_path, _id, _embedId, _pos) => {}

  // ─── @State：场景局部状态 ───
  /** 平台层布局节点（普通节点） */
  @State platformLayoutNodes: PlatformLayoutNode[] = []
  /** 平台层布局节点（Embed） */
  @State embedLayoutNodes: PlatformLayoutNode[] = []
  /** 节点显示矩形列表（含 Embed） */
  @State nodeRects: PlatformNodeRect[] = []
  /** 边渲染几何列表 */
  @State localEdgeRenders: PlatformEdgeRender[] = []
  /** 当前手势模式 */
  @State gestureMode: StarMapGestureMode = 'idle'
  /** 是否显示节点菜单 */
  @State showNodeMenu: boolean = false
  /** 节点菜单对应的 nodeId */
  @State nodeMenuNodeId: string = ''
  /** 预览连线起点 */
  @State previewLineFrom: PlatformPoint = { x: 0, y: 0 }
  /** 预览连线终点 */
  @State previewLineTo: PlatformPoint = { x: 0, y: 0 }
  /** 是否显示新建菜单 */
  @State showCreateMenu: boolean = false
  /** 新建菜单位置（屏幕坐标） */
  @State createMenuPosition: PlatformPoint = { x: 0, y: 0 }
  /** 新建菜单位置（画布坐标） */
  @State createMenuCanvasPosition: PlatformPoint = { x: 0, y: 0 }
  /** 是否显示新建节点对话框 */
  @State showCreateNodeDialog: boolean = false
  /** 是否显示新建子星图对话框 */
  @State showCreateEmbedDialog: boolean = false
  /** 新节点标题 */
  @State newNodeTitle: string = ''
  /** 新子星图标题 */
  @State newEmbedTitle: string = ''
  /** Canvas 是否就绪 */
  @State canvasReady: boolean = false
  /** 预览 Canvas 是否就绪 */
  @State previewCanvasReady: boolean = false
  /** Canvas 是否激活 */
  @State canvasActive: boolean = false

  // ─── 私有状态（非响应式） ───
  private motionPauseTracker: MotionPauseTracker = new MotionPauseTracker()
  private panBaseX: number = 0
  private panBaseY: number = 0
  private moveNodeStartX: number = 0
  private moveNodeStartY: number = 0
  private moveEmbedStartX: number = 0
  private moveEmbedStartY: number = 0
  private pinchActive: boolean = false
  private panActive: boolean = false
  private pinchBaseScale: number = 1
  private pinchBaseOffsetX: number = 0
  private pinchBaseOffsetY: number = 0
  private pinchAnchorCanvasX: number = 0
  private pinchAnchorCanvasY: number = 0

  // ─── Canvas 上下文 ───
  private canvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private canvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.canvasSettings)
  private previewCanvasSettings: RenderingContextSettings = new RenderingContextSettings(true)
  private previewCanvasCtx: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.previewCanvasSettings)

  // ─── 生命周期 ───

  aboutToAppear() {
    this.canvasActive = true
    this.recomputeLayoutAndGeometry()
  }

  aboutToDisappear() {
    this.canvasActive = false
    // 只 reset 属于当前 Scene 的手势状态，不要 reset 别层正在进行的手势
    if (this.gestureTracker.isOwnedByScene(this.scenePath)) {
      this.gestureTracker.reset()
    }
    this.gestureMode = 'idle'
    this.showNodeMenu = false
    this.showCreateMenu = false
    this.nodeMenuNodeId = ''
  }

  // ─── @Watch 处理 ───

  onTransformChange() {
    this.drawEdges()
  }

  onMotionProgressChange() {
    this.drawEdges()
    if (this.gestureMode === 'connect') {
      this.drawPreviewLine()
    }
  }

  onGraphChange() {
    this.recomputeLayoutAndGeometry()
  }

  // ─── 布局与几何计算 ───

  /**
   * 从 graph 重建布局节点和几何数据。
   * 在 graph 变化、节点/Embed 移动后调用。
   */
  recomputeLayoutAndGeometry() {
    if (!this.graph) {
      this.platformLayoutNodes = []
      this.embedLayoutNodes = []
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    this.platformLayoutNodes = buildFreeformLayout(this.graph.nodes)
    this.embedLayoutNodes = buildEmbedLayoutNodes(this.graph.embeds)
    this.recomputeGeometry()
  }

  /**
   * 重新计算几何：nodeRects + localEdgeRenders。
   * 在节点位置变化、graph 加载后调用。
   */
  recomputeGeometry() {
    if (!this.graph) {
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    const nodeRects: PlatformNodeRect[] = this.platformLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    const embedRects: PlatformNodeRect[] = this.embedLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    this.nodeRects = nodeRects.concat(embedRects)
    const rectMap = new Map<string, PlatformNodeRect>()
    for (const rect of this.nodeRects) {
      rectMap.set(rect.nodeId, rect)
    }
    this.localEdgeRenders = computeAllEdgeRenders(this.graph.edges, rectMap, this.starmapId)
    this.drawEdges()
  }

  // ─── 坐标计算 ───

  /**
   * 获取平台层布局节点（普通节点或 Embed）
   */
  private getPlatformLayoutNode(nodeId: string): PlatformLayoutNode | undefined {
    const node = this.platformLayoutNodes.find(ln => ln.nodeId === nodeId)
    return node !== undefined ? node : this.embedLayoutNodes.find(ln => ln.nodeId === nodeId)
  }

  /**
   * 计算节点在屏幕上的最终 x 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenX(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseX = ln ? ln.x : 0
    const wobbleDx = wobbleXFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeItemId)
    return (baseX + wobbleDx) * this.zoomScale + this.offsetX
  }

  /**
   * 计算节点在屏幕上的最终 y 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenY(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseY = ln ? ln.y : 0
    const wobbleDy = wobbleYFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeItemId)
    return (baseY + wobbleDy) * this.zoomScale + this.offsetY
  }

  /**
   * 计算 Embed 在屏幕上的 x 坐标（无 wobble）
   */
  private getEmbedScreenX(embedId: string): number {
    const ln = this.getPlatformLayoutNode(embedId)
    const baseX = ln ? ln.x : 0
    return baseX * this.zoomScale + this.offsetX
  }

  /**
   * 计算 Embed 在屏幕上的 y 坐标
   */
  private getEmbedScreenY(embedId: string): number {
    const ln = this.getPlatformLayoutNode(embedId)
    const baseY = ln ? ln.y : 0
    return baseY * this.zoomScale + this.offsetY
  }

  /**
   * 判断 nodeId 是否是 Embed
   */
  private isEmbed(nodeId: string): boolean {
    if (!this.graph) return false
    return this.graph.embeds.some(e => e.instanceId === nodeId)
  }

  // ─── 手势辅助 ───

  /**
   * 手势触点的屏幕坐标。
   */
  private gestureScreenPoint(
    event: GestureEvent | undefined,
    anchorX: number,
    anchorY: number
  ): PlatformPoint {
    if (!event || !event.fingerList || event.fingerList.length === 0) {
      return { x: anchorX, y: anchorY }
    }
    const finger = event.fingerList[0]
    if (!finger) {
      return { x: anchorX, y: anchorY }
    }
    return {
      x: anchorX + finger.localX,
      y: anchorY + finger.localY
    }
  }

  // ─── Mutation 方法 ───

  /**
   * 构造 item 的 StarMapTargetPath 端点。
   */
  private buildItemTargetPath(itemId: string): StarMapTargetPath {
    if (this.isEmbed(itemId)) {
      return {
        starmapId: this.starmapId,
        segments: [
          { type: 'enterEmbed', instanceId: itemId, nodeId: null }
        ],
        target: {
          type: 'starmap',
          nodeId: null, anchorId: null,
          projectId: null, volumeId: null, chapterId: null,
          rangeStart: null, rangeEnd: null,
          entityType: null, entityId: null, uri: null
        }
      }
    }
    return {
      starmapId: this.starmapId,
      segments: [],
      target: {
        type: 'node',
        nodeId: itemId, anchorId: null,
        projectId: null, volumeId: null, chapterId: null,
        rangeStart: null, rangeEnd: null,
        entityType: null, entityId: null, uri: null
      }
    }
  }

  /**
   * 创建关系（边）：通过 bridge 的星图 CRUD API。
   * 使用 this.starmapId（当前 scene 的 starmapId）。
   */
  async createEdge(fromItemId: string, toItemId: string) {
    hilog.info(0x0000, 'StarMapScene', `createEdge: ${fromItemId} → ${toItemId}`)
    if (!this.bridge) return
    const from = this.buildItemTargetPath(fromItemId)
    const to = this.buildItemTargetPath(toItemId)
    const result = await this.bridge.addStarMapEdge(this.starmapId, from, to, 'RelatedTo', null)
    if (result.success && result.data) {
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: this.graph.nodes,
          edges: [...this.graph.edges, result.data],
          embeds: this.graph.embeds,
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.recomputeGeometry()
      }
      this.onCreateEdge(this.scenePath, this.starmapId, fromItemId, toItemId)
    } else {
      hilog.error(0x0000, 'StarMapScene', `createEdge failed: ${result.errorCode}, ${result.userMessage}`)
    }
  }

  /**
   * 保存节点位置：通过 bridge 的星图节点更新 API 写回。
   * 使用 this.starmapId。
   */
  async saveNodePosition(nodeId: string) {
    const ln = this.getPlatformLayoutNode(nodeId)
    if (!ln || !this.bridge) return
    hilog.info(0x0000, 'StarMapScene', `saveNodePosition: ${nodeId} → (${ln.x}, ${ln.y})`)
    const result = await this.bridge.updateStarMapNode(this.starmapId, nodeId, { x: ln.x, y: ln.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScene', `saveNodePosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
    this.onMoveNode(this.scenePath, this.starmapId, nodeId, { x: ln.x, y: ln.y })
  }

  /**
   * 保存 Embed 位置：通过 bridge 的 Embed update API 写回。
   * 使用 this.starmapId。
   */
  async saveEmbedPosition(embedId: string) {
    const ln = this.getPlatformLayoutNode(embedId)
    if (!ln || !this.bridge) return
    hilog.info(0x0000, 'StarMapScene', `saveEmbedPosition: ${embedId} → (${ln.x}, ${ln.y})`)
    const result = await this.bridge.updateStarMapEmbed(this.starmapId, embedId, { x: ln.x, y: ln.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScene', `saveEmbedPosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
    this.onMoveEmbed(this.scenePath, this.starmapId, embedId, { x: ln.x, y: ln.y })
  }

  /**
   * 执行新建节点：通过 bridge API 创建，使用 this.starmapId。
   */
  async performCreateNode() {
    if (this.newNodeTitle.trim().length === 0) return
    this.showCreateNodeDialog = false
    if (!this.bridge) return
    const position: StarMapPoint = {
      x: this.createMenuCanvasPosition.x,
      y: this.createMenuCanvasPosition.y
    }
    const result = await this.bridge.addStarMapNode(this.starmapId, this.newNodeTitle.trim(), 'Custom', position)
    if (result.success && result.data) {
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: [...this.graph.nodes, result.data],
          edges: this.graph.edges,
          embeds: this.graph.embeds,
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.platformLayoutNodes = buildFreeformLayout(this.graph.nodes)
        this.recomputeGeometry()
      }
      this.selectedNodeId = result.data.id
      this.onSelectItem(this.scenePath, result.data.id)
      this.onCreateNode(this.scenePath, this.starmapId, this.newNodeTitle.trim(), position)
    } else {
      hilog.error(0x0000, 'StarMapScene', `addStarMapNode failed: ${result.errorCode}, ${result.userMessage}`)
    }
    this.newNodeTitle = ''
  }

  /**
   * 执行新建子星图：通过 bridge API 创建，使用 this.starmapId。
   */
  async performCreateEmbed() {
    if (this.newEmbedTitle.trim().length === 0) return
    this.showCreateEmbedDialog = false
    if (!this.bridge) return
    const position: StarMapPoint = {
      x: this.createMenuCanvasPosition.x,
      y: this.createMenuCanvasPosition.y
    }
    const result = await this.bridge.createStarmapChildEmbed(this.starmapId, this.newEmbedTitle.trim(), position)
    if (result.success && result.data) {
      if (this.graph) {
        this.graph = {
          schemaVersion: this.graph.schemaVersion,
          starmapId: this.graph.starmapId,
          nodes: this.graph.nodes,
          edges: this.graph.edges,
          embeds: [...this.graph.embeds, result.data.embed],
          links: this.graph.links,
          hyperlinks: this.graph.hyperlinks
        }
        this.embedLayoutNodes = buildEmbedLayoutNodes(this.graph.embeds)
        this.recomputeGeometry()
      }
      this.onCreateEmbed(this.scenePath, this.starmapId, this.newEmbedTitle.trim(), position)
    } else {
      hilog.error(0x0000, 'StarMapScene', `createStarmapChildEmbed failed: ${result.errorCode ?? 'unknown'}`)
    }
    this.newEmbedTitle = ''
  }

  // ─── Canvas 绘制 ───

  private deactivatePreviewCanvas(): void {
    this.previewCanvasReady = false
  }

  /**
   * 绘制边：使用本地 localEdgeRenders。
   * wobble 偏移在节点位置计算中已包含，此处只需应用 zoom/offset 变换。
   */
  drawEdges() {
    if (!this.canvasActive || !this.canvasReady || !this.graph) return
    const ctx = this.canvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    const progress = this.motionProgress
    const wobbleMap = new Map<string, WobbleOffset>()
    for (const node of this.graph.nodes) {
      const dx = wobbleXFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeItemId)
      const dy = wobbleYFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeItemId)
      wobbleMap.set(node.id, { dx, dy })
    }

    for (const edge of this.localEdgeRenders) {
      const graphEdge = this.graph.edges.find(e => e.id === edge.edgeId)
      if (!graphEdge) continue

      const fromId = resolveEdgeEndpointAnchor(graphEdge.from, this.starmapId) ?? ''
      const toId = resolveEdgeEndpointAnchor(graphEdge.to, this.starmapId) ?? ''
      const defaultWobble: WobbleOffset = { dx: 0, dy: 0 }
      const fromWobble = wobbleMap.get(fromId) || defaultWobble
      const toWobble = wobbleMap.get(toId) || defaultWobble

      const sx = (edge.startX + fromWobble.dx) * this.zoomScale + this.offsetX
      const sy = (edge.startY + fromWobble.dy) * this.zoomScale + this.offsetY
      const ex = (edge.endX + toWobble.dx) * this.zoomScale + this.offsetX
      const ey = (edge.endY + toWobble.dy) * this.zoomScale + this.offsetY

      const isSelected = this.selectedNodeId === fromId || this.selectedNodeId === toId

      ctx.beginPath()
      ctx.moveTo(sx, sy)
      ctx.lineTo(ex, ey)
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.outline
      ctx.lineWidth = isSelected ? 2.5 : 1.5

      if (graphEdge.kind === 'DependsOn' || graphEdge.kind === 'ConflictsWith') {
        ctx.setLineDash([6, 4])
      } else {
        ctx.setLineDash([])
      }
      ctx.stroke()
      ctx.setLineDash([])

      // 箭头
      const atx = (edge.arrowTipX + toWobble.dx) * this.zoomScale + this.offsetX
      const aty = (edge.arrowTipY + toWobble.dy) * this.zoomScale + this.offsetY
      const alx = (edge.arrowLeftX + toWobble.dx) * this.zoomScale + this.offsetX
      const aly = (edge.arrowLeftY + toWobble.dy) * this.zoomScale + this.offsetY
      const arx = (edge.arrowRightX + toWobble.dx) * this.zoomScale + this.offsetX
      const ary = (edge.arrowRightY + toWobble.dy) * this.zoomScale + this.offsetY

      ctx.beginPath()
      ctx.moveTo(atx, aty)
      ctx.lineTo(alx, aly)
      ctx.moveTo(atx, aty)
      ctx.lineTo(arx, ary)
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.outline
      ctx.lineWidth = isSelected ? 2.5 : 1.5
      ctx.stroke()

      // 边标签
      if (graphEdge.label && edge.labelX !== undefined && edge.labelY !== undefined) {
        const lx = (edge.labelX + (fromWobble.dx + toWobble.dx) / 2) * this.zoomScale + this.offsetX
        const ly = (edge.labelY + (fromWobble.dy + toWobble.dy) / 2) * this.zoomScale + this.offsetY
        ctx.font = '11px HarmonyOS Sans'
        ctx.fillStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.onSurfaceVariant
        ctx.textAlign = 'center'
        ctx.textBaseline = 'bottom'
        ctx.fillText(graphEdge.label, lx, ly - 6)
      }
    }
  }

  /**
   * 绘制预览连线：connect 模式下从源节点到当前手指位置。
   */
  drawPreviewLine() {
    if (!this.canvasActive || !this.previewCanvasReady) return
    const ctx = this.previewCanvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    ctx.beginPath()
    ctx.moveTo(this.previewLineFrom.x, this.previewLineFrom.y)
    ctx.lineTo(this.previewLineTo.x, this.previewLineTo.y)
    ctx.strokeStyle = this.themeConfig.colors.primary
    ctx.lineWidth = 2
    ctx.setLineDash([8, 4])
    ctx.stroke()
    ctx.setLineDash([])
  }

  // ─── 节点颜色 ───

  getNodeColor(node: StarMapNode): string {
    const kindColors: Record<string, string> = {
      'Character': '#4A90D9',
      'Location': '#F39C12',
      'Event': '#9B59B6',
      'Theme': '#2ECC71',
      'Plot': '#E74C3C',
      'Item': '#1ABC9C',
      'Organization': '#3498DB',
      'Custom': '#95A5A6'
    }
    return kindColors[node.kind] || '#4A90D9'
  }

  // ─── @Builder：节点组件 ───

  /**
   * 节点组件：长按 → 菜单（连线预备态）→ 继续拖动 → connect。
   * 所有 gestureTracker.begin* 调用传入 this.scenePath 作为 ownerScenePath。
   */
  @Builder
  StarMapNodeItem(node: StarMapNode) {
    Column() {
      Circle({ width: 56 * this.zoomScale, height: 56 * this.zoomScale })
        .fill(this.getNodeColor(node))
        .stroke(this.selectedNodeId === node.id ? '#FF6B6B' : 'transparent')
        .strokeWidth(4)
        .shadow({
          radius: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            DesignTokens.shadowStrong.radius :
            (this.selectedNodeId === node.id ? DesignTokens.shadowMedium.radius : DesignTokens.shadowLight.radius),
          color: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            '#40000000' :
            (this.selectedNodeId === node.id ? '#40FF6B6B' : '#20000000'),
          offsetX: 0,
          offsetY: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id ?
            DesignTokens.shadowStrong.offsetY : DesignTokens.shadowLight.offsetY
        })

      Text(node.title)
        .fontSize(13 * this.zoomScale)
        .fontWeight(this.selectedNodeId === node.id ? FontWeight.Bold : FontWeight.Normal)
        .margin({ top: 6 * this.zoomScale })
        .maxLines(2)
        .textAlign(TextAlign.Center)
        .width(80 * this.zoomScale)
    }
    .width(160 * this.zoomScale)
    .height(80 * this.zoomScale)
    .justifyContent(FlexAlign.Center)
    .position({
      x: this.getNodeScreenX(node.id),
      y: this.getNodeScreenY(node.id)
    })
    .scale(
      this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeItemId === node.id
        ? { x: this.motionConfig.dragLiftScale, y: this.motionConfig.dragLiftScale }
        : { x: 1, y: 1 }
    )
    .onClick(() => {
      this.selectedNodeId = node.id
      this.onSelectItem(this.scenePath, node.id)
    })
    .gesture(
      GestureGroup(GestureMode.Exclusive,
        // 长按 → 菜单（连线预备态）→ 继续拖动 → connect
        GestureGroup(GestureMode.Sequence,
          LongPressGesture({ repeat: false, duration: 500 })
            .onAction((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              this.gestureTracker.beginNodeMenu(this.scenePath, node.id, point.x, point.y)
              this.gestureMode = 'nodeMenu'
              this.showNodeMenu = true
              this.nodeMenuNodeId = node.id
              this.selectedNodeId = node.id
              this.onSelectItem(this.scenePath, node.id)
            }),
          PanGesture()
            .onActionStart((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              this.gestureTracker.beginConnect(this.scenePath, node.id, point.x, point.y)
              this.previewCanvasReady = false
              this.gestureMode = 'connect'
              this.showNodeMenu = false
              const ln = this.getPlatformLayoutNode(node.id)
              if (ln) {
                this.previewLineFrom = {
                  x: (ln.x + ln.width / 2) * this.zoomScale + this.offsetX,
                  y: (ln.y + ln.height / 2) * this.zoomScale + this.offsetY
                }
              }
              this.previewLineTo = point
            })
            .onActionUpdate((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              const state = this.gestureTracker.getState()
              if (state.mode === 'connect') {
                this.previewLineTo = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
                this.drawPreviewLine()
              }
            })
            .onActionEnd(() => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              const state = this.gestureTracker.getState()
              if (state.mode === 'connect') {
                const screenX = this.previewLineTo.x
                const screenY = this.previewLineTo.y
                const canvasPoint = screenToCanvas(screenX, screenY, this.zoomScale, this.offsetX, this.offsetY)
                const targetItemId = hitTestWithScene(this.nodeRects, canvasPoint.x, canvasPoint.y, this.scenePath, this.getEmbedInstanceIds())
                if (targetItemId !== null && targetItemId.objectId !== node.id) {
                  if (targetItemId.objectKind === 'embedInnerContent') {
                    // 父 Scene 不创建 edge，留给 child Scene 处理
                  } else {
                    void this.createEdge(node.id, targetItemId.objectId)
                  }
                }
              }
              this.gestureTracker.reset()
              this.deactivatePreviewCanvas()
              this.gestureMode = 'idle'
              this.showNodeMenu = false
              this.nodeMenuNodeId = ''
            })
        ),
        // 没有长按成功时的普通滑动
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveNode' && state.activeItemId === node.id) {
              this.motionPauseTracker.pauseNode(node.id)
              const ln = this.getPlatformLayoutNode(node.id)
              if (ln) {
                this.moveNodeStartX = ln.x
                this.moveNodeStartY = ln.y
              }
            } else {
              if (this.pinchActive) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              this.panActive = true
              const point = this.gestureScreenPoint(event, 0, 0)
              this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
              this.gestureMode = 'panCanvas'
              this.panBaseX = this.offsetX
              this.panBaseY = this.offsetY
            }
          })
          .onActionUpdate((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveNode' && state.activeItemId === node.id) {
              const newDx = this.moveNodeStartX + event.offsetX / this.zoomScale
              const newDy = this.moveNodeStartY + event.offsetY / this.zoomScale
              const positions = new Map<string, PlatformPoint>()
              positions.set(node.id, { x: newDx, y: newDy })
              this.platformLayoutNodes = applyFreeformLayout(this.platformLayoutNodes, positions)
              this.recomputeGeometry()
            } else if (state.mode === 'panCanvas' && this.panActive && !this.pinchActive) {
              this.offsetX = this.panBaseX + event.offsetX
              this.offsetY = this.panBaseY + event.offsetY
            }
          })
          .onActionEnd(() => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveNode' && state.activeItemId === node.id) {
              void this.saveNodePosition(node.id)
              this.motionPauseTracker.resumeNode(node.id)
            }
            this.panActive = false
            this.gestureTracker.reset()
            this.gestureMode = 'idle'
          })
      )
    )
  }

  // ─── Embed 手势组 ───

  /**
   * Embed 手势组：选中/菜单/连线/移动（不含画布拖动）。
   * title Row 和边框命中条共享此手势组。
   * Pan 手势只处理 moveEmbed，不处理画布拖动——只有空白区域才应该触发画布拖动。
   * 所有 onActionStart 检查 isOwnedByScene，防止别层正在用手势时覆盖 owner。
   */
  private embedGestureGroup(embed: StarMapEmbed): GestureGroup {
    return GestureGroup(GestureMode.Exclusive,
      // 单击 → 选中 Embed
      TapGesture({ count: 1 })
        .onAction(() => {
          this.selectedNodeId = embed.instanceId
          this.onSelectItem(this.scenePath, embed.instanceId)
        }),
      // 长按 → 菜单（连线预备态）→ 继续拖动 → connect
      GestureGroup(GestureMode.Sequence,
        LongPressGesture({ repeat: false, duration: 500 })
          .onAction((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            const point = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
            this.gestureTracker.beginNodeMenu(this.scenePath, embed.instanceId, point.x, point.y)
            this.gestureMode = 'nodeMenu'
            this.showNodeMenu = true
            this.nodeMenuNodeId = embed.instanceId
          }),
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
            const point = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
            this.gestureTracker.beginConnect(this.scenePath, embed.instanceId, point.x, point.y)
            this.previewCanvasReady = false
            this.gestureMode = 'connect'
            this.showNodeMenu = false
            const ln = this.getPlatformLayoutNode(embed.instanceId)
            if (ln) {
              this.previewLineFrom = {
                x: (ln.x + ln.width / 2) * this.zoomScale + this.offsetX,
                y: (ln.y + ln.height / 2) * this.zoomScale + this.offsetY
              }
            }
            this.previewLineTo = point
          })
          .onActionUpdate((event: GestureEvent) => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'connect') {
              this.previewLineTo = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
              this.drawPreviewLine()
            }
          })
          .onActionEnd(() => {
            if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
            const state = this.gestureTracker.getState()
            if (state.mode === 'connect') {
              const screenX = this.previewLineTo.x
              const screenY = this.previewLineTo.y
              const canvasPoint = screenToCanvas(screenX, screenY, this.zoomScale, this.offsetX, this.offsetY)
              const targetItemId = hitTestWithScene(this.nodeRects, canvasPoint.x, canvasPoint.y, this.scenePath, this.getEmbedInstanceIds())
              if (targetItemId !== null && targetItemId.objectId !== embed.instanceId) {
                if (targetItemId.objectKind === 'embedInnerContent') {
                  // 父 Scene 不创建 edge，留给 child Scene 处理
                } else {
                  void this.createEdge(embed.instanceId, targetItemId.objectId)
                }
              }
            }
            this.gestureTracker.reset()
            this.deactivatePreviewCanvas()
            this.gestureMode = 'idle'
            this.showNodeMenu = false
            this.nodeMenuNodeId = ''
          })
      ),
      // 没有长按成功时的普通滑动（只处理 moveEmbed，不处理画布拖动）
      PanGesture()
        .onActionStart((event: GestureEvent) => {
          if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
          const state = this.gestureTracker.getState()
          if (state.mode === 'moveEmbed' && state.activeItemId === embed.instanceId) {
            this.motionPauseTracker.pauseNode(embed.instanceId)
            const ln = this.getPlatformLayoutNode(embed.instanceId)
            if (ln) {
              this.moveEmbedStartX = ln.x
              this.moveEmbedStartY = ln.y
            }
          }
        })
        .onActionUpdate((event: GestureEvent) => {
          if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
          const state = this.gestureTracker.getState()
          if (state.mode === 'moveEmbed' && state.activeItemId === embed.instanceId) {
            const newDx = this.moveEmbedStartX + event.offsetX / this.zoomScale
            const newDy = this.moveEmbedStartY + event.offsetY / this.zoomScale
            const positions = new Map<string, PlatformPoint>()
            positions.set(embed.instanceId, { x: newDx, y: newDy })
            this.embedLayoutNodes = applyFreeformLayout(this.embedLayoutNodes, positions)
            this.recomputeGeometry()
          }
        })
        .onActionEnd(() => {
          if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
          const state = this.gestureTracker.getState()
          if (state.mode === 'moveEmbed' && state.activeItemId === embed.instanceId) {
            void this.saveEmbedPosition(embed.instanceId)
            this.motionPauseTracker.resumeNode(embed.instanceId)
          }
          this.gestureTracker.reset()
          this.gestureMode = 'idle'
        })
    )
  }

  // ─── @Builder：Embed 组件 ───

  /**
   * Embed 组件：使用 Stack 分离命中层。
   * - innerContent（底层）：放 child StarMapEmbedScene，不挂任何父手势
   * - title Row（上层）：挂 Embed 手势（选中/菜单/连线/移动）
   * - 左/右/下边框命中条（上层）：透明窄条，挂相同 Embed 手势
   * Stack 中后声明的组件在视觉和触摸测试中优先级更高，确保 title/边框优先于 innerContent。
   */
  @Builder
  StarMapEmbedItem(embed: StarMapEmbed) {
    Stack() {
      // 1. innerContent 区域（占满整个 Embed 内容区，放 child StarMapEmbedScene）
      // 不挂任何手势，事件穿透到 child Scene
      Column() {
        StarMapEmbedScene({
          rootStarmapId: this.rootStarmapId,
          scenePath: this.scenePath,
          embed: embed,
          sceneDepth: this.sceneDepth,
          themeConfig: this.themeConfig,
          motionConfig: this.motionConfig,
          motionProgress: this.motionProgress,
          bridge: this.bridge,
          gestureTracker: this.gestureTracker,
        })
      }
      .width('100%')
      .height('100%')

      // 2. title Row（顶部条，挂 Embed 手势：选中/菜单/连线/移动）
      Row() {
        Text(embed.label ?? embed.targetStarmapId)
          .fontSize(14 * this.zoomScale)
          .fontWeight(FontWeight.Medium)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      }
      .width('100%')
      .height(24 * this.zoomScale)
      .padding({ left: 8 * this.zoomScale, right: 8 * this.zoomScale })
      .justifyContent(FlexAlign.Start)
      .alignItems(VerticalAlign.Center)
      .position({ x: 0, y: 0 })
      .gesture(this.embedGestureGroup(embed))

      // 3. 左边框命中条（透明窄条，只接收手势事件）
      Row() {}
      .width(4)
      .height('100%')
      .position({ x: 0, y: 0 })
      .gesture(this.embedGestureGroup(embed))

      // 4. 右边框命中条（透明窄条，只接收手势事件）
      Row() {}
      .width(4)
      .height('100%')
      .position({ x: 200 * this.zoomScale - 4, y: 0 })
      .gesture(this.embedGestureGroup(embed))

      // 5. 下边框命中条（透明窄条，只接收手势事件）
      Row() {}
      .width('100%')
      .height(4)
      .position({ x: 0, y: 120 * this.zoomScale - 4 })
      .gesture(this.embedGestureGroup(embed))
    }
    .width(200 * this.zoomScale)
    .height(120 * this.zoomScale)
    .borderRadius(12 * this.zoomScale)
    .backgroundColor('#E8E8E8')
    .border({
      width: this.selectedNodeId === embed.instanceId ? 3 : 2,
      color: this.selectedNodeId === embed.instanceId
        ? this.themeConfig.colors.primary
        : this.themeConfig.colors.outlineVariant
    })
    .position({
      x: this.getEmbedScreenX(embed.instanceId),
      y: this.getEmbedScreenY(embed.instanceId)
    })
    .clip(true)
  }

  // ─── @Builder：节点菜单 ───

  /**
   * 节点菜单：长按节点/Embed 后弹出。
   * 普通节点和 Embed 分开菜单：Embed 只有"移动子星图"，普通节点只有"移动节点"。
   */
  @Builder
  NodeMenu() {
    Column() {
      Text(this.getPlatformLayoutNode(this.nodeMenuNodeId)?.nodeId ?? '')
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 8 })

      if (this.isEmbed(this.nodeMenuNodeId)) {
        Button($r('app.string.move_embed'))
          .fontSize(13)
          .height(36)
          .width('100%')
          .onClick(() => {
            this.gestureTracker.beginMoveEmbed(this.scenePath, this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveEmbed'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })
      } else {
        Button($r('app.string.move_node'))
          .fontSize(13)
          .height(36)
          .width('100%')
          .onClick(() => {
            this.gestureTracker.beginMoveNode(this.scenePath, this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveNode'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })
      }

      Button($r('app.string.cancel'))
        .fontSize(13)
        .height(36)
        .width('100%')
        .margin({ top: 4 })
        .onClick(() => {
          if (this.gestureTracker.isOwnedByScene(this.scenePath)) {
            this.gestureTracker.reset()
          }
          this.gestureMode = 'idle'
          this.showNodeMenu = false
          this.nodeMenuNodeId = ''
        })
    }
    .width(160)
    .padding(12)
    .borderRadius(DesignTokens.radiusMd)
    .shadow({ radius: DesignTokens.shadowMedium.radius, offsetX: 0, offsetY: DesignTokens.shadowMedium.offsetY })
    .backgroundColor('#FFFFFF')
    .position({
      x: this.getNodeScreenX(this.nodeMenuNodeId) + 80,
      y: this.getNodeScreenY(this.nodeMenuNodeId) - 40
    })
  }

  // ─── @Builder：新建菜单 ───

  @Builder
  CreateMenu() {
    Column() {
      Button('新建节点')
        .fontSize(13)
        .height(36)
        .width('100%')
        .onClick(() => {
          this.showCreateMenu = false
          this.showCreateNodeDialog = true
          this.newNodeTitle = ''
        })

      Button('新建子星图')
        .fontSize(13)
        .height(36)
        .width('100%')
        .margin({ top: 4 })
        .onClick(() => {
          this.showCreateMenu = false
          this.showCreateEmbedDialog = true
          this.newEmbedTitle = ''
        })

      Button($r('app.string.cancel'))
        .fontSize(13)
        .height(36)
        .width('100%')
        .margin({ top: 4 })
        .onClick(() => {
          this.showCreateMenu = false
        })
    }
    .width(160)
    .padding(12)
    .borderRadius(DesignTokens.radiusMd)
    .shadow({ radius: DesignTokens.shadowMedium.radius, offsetX: 0, offsetY: DesignTokens.shadowMedium.offsetY })
    .backgroundColor($r('sys.color.ohos_id_color_card_bg'))
    .position({
      x: this.createMenuPosition.x,
      y: this.createMenuPosition.y
    })
  }

  // ─── @Builder：新建节点对话框 ───

  @Builder
  CreateNodeDialog() {
    Column() {
      Column() {
        Text('新建节点')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ placeholder: '请输入节点名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .onChange((value: string) => {
            this.newNodeTitle = value
          })

        Row() {
          Button($r('app.string.cancel'))
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.showCreateNodeDialog = false
              this.newNodeTitle = ''
            })

          Blank()

          Button('创建')
            .fontSize(14)
            .height(36)
            .enabled(this.newNodeTitle.trim().length > 0)
            .onClick(() => {
              void this.performCreateNode()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('80%')
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }

  // ─── @Builder：新建子星图对话框 ───

  @Builder
  CreateEmbedDialog() {
    Column() {
      Column() {
        Text('新建子星图')
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
          .margin({ bottom: 16 })

        TextInput({ placeholder: '请输入子星图名称' })
          .width('100%')
          .height(40)
          .fontSize(16)
          .onChange((value: string) => {
            this.newEmbedTitle = value
          })

        Row() {
          Button($r('app.string.cancel'))
            .fontSize(14)
            .height(36)
            .onClick(() => {
              this.showCreateEmbedDialog = false
              this.newEmbedTitle = ''
            })

          Blank()

          Button('创建')
            .fontSize(14)
            .height(36)
            .enabled(this.newEmbedTitle.trim().length > 0)
            .onClick(() => {
              void this.performCreateEmbed()
            })
        }
        .width('100%')
        .margin({ top: 16 })
      }
      .width('80%')
      .padding(20)
      .borderRadius(DesignTokens.radiusLg)
      .backgroundColor($r('sys.color.background_secondary'))
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#80000000')
    .justifyContent(FlexAlign.Center)
  }

  // ─── 辅助方法 ───

  /**
   * 获取当前星图所有 Embed 的 instanceId 集合，用于 hitTestWithScene 区分节点和 Embed。
   */
  private getEmbedInstanceIds(): Set<string> {
    if (!this.graph) return new Set()
    const ids = new Set<string>()
    for (const embed of this.graph.embeds) {
      ids.add(embed.instanceId)
    }
    return ids
  }

  // ─── build ───

  build() {
    Stack() {
      // 0. 背景命中层（只处理当前 Scene 空白区域的 Pinch/Pan/LongPress）
      // 位于 Stack 最底层，子组件在上层优先接收触摸事件
      // 空白区域的触摸事件穿透到此层
      Column() {}
      .width('100%')
      .height('100%')
      .gesture(
        // 空白区域手势：Pinch（双指缩放）+ Pan（画布拖动）+ LongPress（长按新建）
        GestureGroup(GestureMode.Parallel,
          PinchGesture()
            .onActionStart((event: GestureEvent) => {
              this.pinchActive = true
              this.panActive = false
              this.pinchBaseScale = this.zoomScale
              this.pinchBaseOffsetX = this.offsetX
              this.pinchBaseOffsetY = this.offsetY
              // 计算双指中心的画布坐标作为缩放锚点
              if (event.fingerList && event.fingerList.length >= 2) {
                const f0 = event.fingerList[0]
                const f1 = event.fingerList[1]
                if (f0 && f1) {
                  const centerX = (f0.localX + f1.localX) / 2
                  const centerY = (f0.localY + f1.localY) / 2
                  const canvasPoint = screenToCanvas(centerX, centerY, this.zoomScale, this.offsetX, this.offsetY)
                  this.pinchAnchorCanvasX = canvasPoint.x
                  this.pinchAnchorCanvasY = canvasPoint.y
                }
              }
            })
            .onActionUpdate((event: GestureEvent) => {
              if (!this.pinchActive) return
              const rawScale = this.pinchBaseScale * event.scale
              const MIN_ZOOM = 0.3
              const MAX_ZOOM = 3.0
              const nextScale = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, rawScale))
              // 跟随双指中心：使用 event.pinchCenterX/Y 作为屏幕参考点
              // offsetX = centerX - anchorCanvasX * nextScale
              // offsetY = centerY - anchorCanvasY * nextScale
              const centerX = event.pinchCenterX
              const centerY = event.pinchCenterY
              this.offsetX = centerX - this.pinchAnchorCanvasX * nextScale
              this.offsetY = centerY - this.pinchAnchorCanvasY * nextScale
              this.zoomScale = nextScale
            })
            .onActionEnd(() => {
              this.pinchActive = false
            }),
          PanGesture()
            .onActionStart((event: GestureEvent) => {
              if (this.pinchActive) return
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              this.panActive = true
              const point = this.gestureScreenPoint(event, 0, 0)
              this.gestureTracker.beginPanCanvas(this.scenePath, point.x, point.y)
              this.gestureMode = 'panCanvas'
              this.panBaseX = this.offsetX
              this.panBaseY = this.offsetY
            })
            .onActionUpdate((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              if (!this.panActive || this.pinchActive) return
              if (this.gestureMode === 'panCanvas') {
                this.offsetX = this.panBaseX + event.offsetX
                this.offsetY = this.panBaseY + event.offsetY
              }
            })
            .onActionEnd(() => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath)) return
              this.panActive = false
              this.gestureTracker.reset()
              this.gestureMode = 'idle'
            }),
          LongPressGesture({ repeat: false, duration: 500 })
            .onAction((event: GestureEvent) => {
              if (!this.gestureTracker.isOwnedByScene(this.scenePath) && !this.gestureTracker.isIdle()) return
              const point = this.gestureScreenPoint(event, 0, 0)
              const canvasPoint = screenToCanvas(point.x, point.y, this.zoomScale, this.offsetX, this.offsetY)
              const hitResult = hitTestWithScene(this.nodeRects, canvasPoint.x, canvasPoint.y, this.scenePath, this.getEmbedInstanceIds())
              // 空白区域长按 → 弹出新建菜单
              if (hitResult === null) {
                this.showCreateMenu = true
                this.createMenuPosition = { x: point.x, y: point.y }
                this.createMenuCanvasPosition = { x: canvasPoint.x, y: canvasPoint.y }
              }
            })
        )
      )
      .hitTestBehavior(HitTestMode.Default)

      // 1. Canvas：绘制当前层的边
      Canvas(this.canvasCtx)
        .width('100%')
        .height('100%')
        .onReady(() => {
          this.canvasReady = true
          this.drawEdges()
        })

      // 2. 节点层
      if (this.graph) {
        ForEach(this.graph.nodes, (node: StarMapNode) => {
          this.StarMapNodeItem(node)
        }, (node: StarMapNode) => node.id)

        // 3. Embed 层
        ForEach(this.graph.embeds, (embed: StarMapEmbed) => {
          this.StarMapEmbedItem(embed)
        }, (embed: StarMapEmbed) => embed.instanceId)
      }

      // 4. 预览连线 Canvas（connect 模式）
      if (this.gestureMode === 'connect') {
        Canvas(this.previewCanvasCtx)
          .width('100%')
          .height('100%')
          .onReady(() => {
            this.previewCanvasReady = true
            this.drawPreviewLine()
          })
      }

      // 5. 节点菜单
      if (this.showNodeMenu) {
        this.NodeMenu()
      }

      // 6. 新建菜单
      if (this.showCreateMenu) {
        this.CreateMenu()
      }

      // 7. 新建节点对话框
      if (this.showCreateNodeDialog) {
        this.CreateNodeDialog()
      }

      // 8. 新建子星图对话框
      if (this.showCreateEmbedDialog) {
        this.CreateEmbedDialog()
      }
    }
    .width('100%')
    .height('100%')
  }
}
