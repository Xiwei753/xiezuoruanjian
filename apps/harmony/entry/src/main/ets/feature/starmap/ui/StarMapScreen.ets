import { Routes, StarMapRouteParams } from '../../../app/navigation/Routes'
import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed } from '../../../corebridge/dto/StarMapDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { WidthClass, ScreenRole, ActionSlot } from '../../../corebridge/dto/PlatformDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { HarmonyShareService } from '../../../platform/share/HarmonyShareService'
import { SharePayload } from '../../../platform/share/SharePayload'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { ScreenPolicyBridge } from '../../../ui/adaptive/ScreenPolicyBridge'
import { PlatformNodeRect, PlatformEdgeRender, deriveNodeRectFromLayoutNode, hitTest, computeAllEdgeRenders, screenToCanvas } from '../platform/StarMapGeometry'
import { PlatformLayoutNode, PlatformPoint, NodePositionPatch, buildFreeformLayout, buildEmbedLayoutNodes, applyFreeformLayout, autoGridLayoutNodes, generatePositionPatches } from '../platform/StarMapLayout'
import { PlatformMotionConfig, defaultMotionConfig, wobbleXFromProgress, wobbleYFromProgress, MotionPauseTracker, shouldAnimate } from '../platform/StarMapMotion'
import Animator, { AnimatorResult } from '@ohos.animator'
import { StarMapGestureMode, StarMapGestureStateTracker } from '../platform/StarMapGestureState'

/** wobble 视觉偏移（dx, dy）：只影响显示，不写回 Core。 */
interface WobbleOffset {
  dx: number
  dy: number
}

// 缩放边界：缩到 MIN 之下即视为"退出当前层"，回到父星图（#789：缩放返回父层）
const MIN_ZOOM: number = 0.3
const MAX_ZOOM: number = 3
const ZOOM_STEP: number = 0.1

@Component
export struct StarMapScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State starmapId: string = ''
  @State starmapTitle: string = ''
  @State parentStarmapId: string = ''
  @State parentStarmapTitle: string = ''
  @State graph: StarMapGraph | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State @Watch('onTransformChange') selectedNodeId: string = ''
  @State selectedNode: StarMapNode | null = null
  @State @Watch('onTransformChange') zoomScale: number = 1
  @State @Watch('onTransformChange') offsetX: number = 0
  @State @Watch('onTransformChange') offsetY: number = 0
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  @State useHdsNav: boolean = false
  @State headerActions: ActionSlot[] = []

  // 平台层布局
  @State platformLayoutNodes: PlatformLayoutNode[] = []
  @State embedLayoutNodes: PlatformLayoutNode[] = []
  @State nodeRects: PlatformNodeRect[] = []
  @State localEdgeRenders: PlatformEdgeRender[] = []

  // 运动配置（本地默认值，不从 Core 获取）
  @State motionConfig: PlatformMotionConfig = defaultMotionConfig()
  private motionPauseTracker: MotionPauseTracker = new MotionPauseTracker()

  // 手势状态
  private gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  @State gestureMode: StarMapGestureMode = 'idle'
  @State showNodeMenu: boolean = false
  @State nodeMenuNodeId: string = ''
  @State previewLineFrom: PlatformPoint = { x: 0, y: 0 }
  @State previewLineTo: PlatformPoint = { x: 0, y: 0 }

  // 动画驱动：用 Animator onFrame 逐帧回调驱动 motionProgress ∈ [0, 1]
  // progress 直接映射到 wobble 的 sin/cos 相位，每帧更新节点位置和 Canvas 边
  @State @Watch('onMotionProgressChange') motionProgress: number = 0
  private wobbleAnimator: AnimatorResult | null = null

  // 双指缩放的基准比例（手势开始时记录）
  private pinchBaseScale: number = 1

  // 画布拖动基准（onActionStart 记录，onActionUpdate 用 base+offset 避免重复累加）
  private panBaseX: number = 0
  private panBaseY: number = 0

  // 节点移动基准（onActionStart 记录，onActionUpdate 用 start+offset/zoom 避免重复累加）
  private moveNodeStartX: number = 0
  private moveNodeStartY: number = 0

  // Embed 移动基准（同节点移动）
  private moveEmbedStartX: number = 0
  private moveEmbedStartY: number = 0

  // Stack 容器尺寸（用于 position 数值计算）
  @State stackWidth: number = 0
  @State stackHeight: number = 0

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  // 分享服务（碰一碰/隔空传送/系统分享统一入口）。
  private shareService: HarmonyShareService = HarmonyShareService.getInstance()

  async aboutToAppear() {
    hilog.info(0x0000, 'StarMapScreen', 'route name=StarMap')
    const paramsArr = this.navPathStack.getParamByName(Routes.StarMap) as StarMapRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.starmapId = params.starmapId
      this.starmapTitle = params.starmapTitle
      this.parentStarmapId = params.parentStarmapId ?? ''
      this.parentStarmapTitle = params.parentStarmapTitle ?? ''
      this.loadStarMap()
    } else {
      // 没有路由参数时不自动选择第一张星图，直接显示空状态
      this.isLoading = false
    }

    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    if (!this.useHdsNav) {
      const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(ScreenRole.StarMap)
      if (policy !== null) {
        this.headerActions = policy.actionSlots
      }
    }

    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver)
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()

    // 启动 wobble 动画：用 Animator onFrame 逐帧回调驱动 motionProgress
    this.startWobbleAnimation()

    void this.startShareLifecycle()
  }

  aboutToDisappear() {
    // 页面退出时 cancel Animator，避免动画继续持有页面状态
    this.stopWobbleAnimation()
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    void this.stopShareLifecycle()
  }

  /**
   * 用 ArkUI Animator 的 onFrame 逐帧回调驱动 motionProgress ∈ [0, 1]。
   * Animator 的 begin=0, end=1, iterations=-1（无限循环），easing='linear'。
   * onFrame 每帧把 progress 写入 @State motionProgress，触发 @Watch 重画 Canvas 边。
   * 节点位置也从同一个 motionProgress 派生 wobble 偏移，保证边和节点同步。
   */
  startWobbleAnimation() {
    if (!shouldAnimate(this.motionConfig)) {
      return
    }
    this.wobbleAnimator = Animator.create({
      duration: this.motionConfig.idlePeriodMs,
      iterations: -1,
      easing: 'linear',
      begin: 0,
      end: 1,
      delay: 0,
      fill: 'none',
      direction: 'normal'
    })
    this.wobbleAnimator.onFrame = (progress: number) => {
      this.motionProgress = progress
    }
    this.wobbleAnimator.play()
  }

  /**
   * 停止 wobble Animator，释放动画资源。
   * 页面退出时调用，避免动画继续持有页面状态。
   */
  stopWobbleAnimation() {
    if (this.wobbleAnimator) {
      this.wobbleAnimator.cancel()
      this.wobbleAnimator = null
    }
  }

  async loadStarMap() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.getStarMapGraph(this.starmapId)
      if (result.success && result.data) {
        this.graph = result.data
        // Freeform 布局：直接从 graph.nodes 的 authored position 构建布局节点
        this.platformLayoutNodes = buildFreeformLayout(result.data.nodes)
        // 构建 Embed 布局节点
        this.embedLayoutNodes = buildEmbedLayoutNodes(result.data.embeds)
        // 本地计算 nodeRects 和 edgeRenders（不再从 Core 获取）
        this.recomputeGeometry()
      } else {
        this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_starmap').id)
      }
    } catch (err) {
      this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_starmap_detail').id)
    } finally {
      this.isLoading = false
    }

    this.updateSharePayload()
  }

  /**
   * 自动网格布局：调用平台层的 autoGridLayoutNodes，把结果应用到现有节点。
   * 只在用户明确点"自动布局"按钮时调用；加载页面不排网格。
   */
  applyAutoLayout() {
    if (this.platformLayoutNodes.length === 0 || this.stackWidth === 0) {
      return
    }
    this.platformLayoutNodes = autoGridLayoutNodes(this.platformLayoutNodes, this.stackWidth, this.stackHeight)
    this.recomputeGeometry()
    // 自动布局后把结果通过 position patch 写回 Core
    this.saveAllNodePositions()
  }

  /**
   * 保存所有节点位置到 Core（自动布局后批量写回 position patch）。
   */
  async saveAllNodePositions() {
    if (!this.graph) return
    const patches: NodePositionPatch[] = generatePositionPatches(this.platformLayoutNodes)
    for (const patch of patches) {
      const result = await this.bridge.updateStarMapNode(this.starmapId, patch.nodeId, { x: patch.x, y: patch.y })
      if (!result.success) {
        hilog.error(0x0000, 'StarMapScreen', `saveAllNodePositions ${patch.nodeId} failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
      }
    }
  }

  /**
   * 重新计算所有几何：nodeRects + localEdgeRenders。
   * 在节点位置变化、graph 加载后调用。
   * 几何重算后主动重画边 Canvas，否则节点移动时旧连线不会跟着动。
   */
  recomputeGeometry() {
    if (!this.graph) {
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    // 节点与 Embed 都从平台层布局节点派生显示矩形
    const nodeRects: PlatformNodeRect[] = this.platformLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    const embedRects: PlatformNodeRect[] = this.embedLayoutNodes.map((ln: PlatformLayoutNode) => deriveNodeRectFromLayoutNode(ln))
    this.nodeRects = nodeRects.concat(embedRects)
    // 构建 nodeId → PlatformNodeRect 映射
    const rectMap = new Map<string, PlatformNodeRect>()
    for (const rect of this.nodeRects) {
      rectMap.set(rect.nodeId, rect)
    }
    // 本地计算 edgeRenders
    this.localEdgeRenders = computeAllEdgeRenders(this.graph.edges, rectMap)
    // 几何重算后主动重画边 Canvas（canvasCtx 可能还未初始化，drawEdges 内部有 null 检查）
    this.drawEdges()
  }

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

  getNodeTypeName(kind: string): string {
    const kindNames: Record<string, string> = {
      'Character': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_character').id),
      'Location': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_location').id),
      'Event': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_event').id),
      'Theme': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_theme').id),
      'Plot': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_plot').id),
      'Item': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_item').id),
      'Organization': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_organization').id),
      'Custom': getContext(this).resourceManager.getStringSync($r('app.string.node_kind_custom').id)
    }
    return kindNames[kind] || getContext(this).resourceManager.getStringSync($r('app.string.node_kind_unknown').id)
  }

  getEdgeTypeName(kind: string): string {
    const kindNames: Record<string, string> = {
      'CharacterRelation': getContext(this).resourceManager.getStringSync($r('app.string.edge_kind_character_relation').id),
      'DependsOn': getContext(this).resourceManager.getStringSync($r('app.string.edge_kind_depends_on').id),
      'Timeline': getContext(this).resourceManager.getStringSync($r('app.string.edge_kind_timeline').id),
      'ConflictsWith': getContext(this).resourceManager.getStringSync($r('app.string.edge_kind_conflicts_with').id),
      'RelatedTo': getContext(this).resourceManager.getStringSync($r('app.string.edge_kind_related_to').id),
      'Custom': getContext(this).resourceManager.getStringSync($r('app.string.edge_kind_custom').id)
    }
    return kindNames[kind] || getContext(this).resourceManager.getStringSync($r('app.string.edge_kind_unknown').id)
  }

  selectNode(nodeId: string) {
    this.selectedNodeId = nodeId
    this.selectedNode = this.graph?.nodes.find(n => n.id === nodeId) || null
  }

  deselectNode() {
    this.selectedNodeId = ''
    this.selectedNode = null
  }

  /**
   * 手势触点的屏幕坐标。
   * GestureEvent 本身没有 x/y：触点坐标在 `fingerList[0].localX/localY`（相对触发组件
   * 左上角），加上组件自身的屏幕位置才是屏幕坐标。
   */
  private gestureScreenPoint(event: GestureEvent, anchorX: number, anchorY: number): PlatformPoint {
    const fingers: FingerInfo[] = event.fingerList
    if (fingers && fingers.length > 0) {
      return { x: anchorX + fingers[0].localX, y: anchorY + fingers[0].localY }
    }
    return { x: anchorX, y: anchorY }
  }

  /**
   * 获取平台层布局节点（普通节点或 Embed：两者都在同一套 PlatformLayoutNode 上）
   */
  private getPlatformLayoutNode(nodeId: string): PlatformLayoutNode | undefined {
    const node = this.platformLayoutNodes.find(ln => ln.nodeId === nodeId)
    return node !== undefined ? node : this.embedLayoutNodes.find(ln => ln.nodeId === nodeId)
  }

  /**
   * 按屏幕位移移动布局节点，返回新的布局数组（applyFreeformLayout 不原地改）。
   */
  private moveLayoutNode(layoutNodes: PlatformLayoutNode[], nodeId: string,
    screenDx: number, screenDy: number): PlatformLayoutNode[] {
    const ln = layoutNodes.find(n => n.nodeId === nodeId)
    if (!ln) {
      return layoutNodes
    }
    const positions = new Map<string, PlatformPoint>()
    positions.set(nodeId, { x: ln.x + screenDx / this.zoomScale, y: ln.y + screenDy / this.zoomScale })
    return applyFreeformLayout(layoutNodes, positions)
  }

  /** moveNode 模式：Pan 画布位移 → 节点布局坐标 */
  private moveNodeBy(nodeId: string, screenDx: number, screenDy: number) {
    this.platformLayoutNodes = this.moveLayoutNode(this.platformLayoutNodes, nodeId, screenDx, screenDy)
    this.recomputeGeometry()
  }

  /** moveEmbed 模式：Pan 画布位移 → Embed 布局坐标 */
  private moveEmbedBy(embedId: string, screenDx: number, screenDy: number) {
    this.embedLayoutNodes = this.moveLayoutNode(this.embedLayoutNodes, embedId, screenDx, screenDy)
    this.recomputeGeometry()
  }

  /**
   * 计算节点在屏幕上的最终 x 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenX(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseX = ln ? ln.x : 0
    const wobbleDx = wobbleXFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeNodeId)
    return (baseX + wobbleDx) * this.zoomScale + this.offsetX
  }

  /**
   * 计算节点在屏幕上的最终 y 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenY(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseY = ln ? ln.y : 0
    const wobbleDy = wobbleYFromProgress(nodeId, this.motionProgress, this.motionConfig, this.gestureTracker.getState().activeNodeId)
    return (baseY + wobbleDy) * this.zoomScale + this.offsetY
  }

  /**
   * 计算 Embed 在屏幕上的 x 坐标（无 wobble：Embed 位置由用户显式移动）
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

  /**
   * 保存 Embed 位置：通过 bridge 的 Embed update API 把 position 写回 Core。
   * Embed 位置只读 embed.position，显示宽高圆角由平台层定义。
   */
  async saveEmbedPosition(embedId: string) {
    const ln = this.getPlatformLayoutNode(embedId)
    if (!ln) return
    hilog.info(0x0000, 'StarMapScreen', `saveEmbedPosition: ${embedId} → (${ln.x}, ${ln.y})`)
    const result = await this.bridge.updateStarMapEmbed(this.starmapId, embedId, { x: ln.x, y: ln.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScreen', `saveEmbedPosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
  }

  /**
   * 导航到父星图（"返回父星图"入口与"缩放返回父层"共用同一条路径）。
   */
  returnToParent() {
    if (this.parentStarmapId.length === 0) {
      return
    }
    this.navPathStack.pop()
  }

  /** 放大一级（工具栏 +） */
  zoomIn() {
    this.zoomScale = Math.min(MAX_ZOOM, this.zoomScale + ZOOM_STEP)
  }

  /** 缩小一级（工具栏 −）；已经缩到最小时缩出当前层，回到父星图 */
  zoomOut() {
    if (this.zoomScale - ZOOM_STEP < MIN_ZOOM && this.parentStarmapId.length > 0) {
      this.returnToParent()
      return
    }
    this.zoomScale = Math.max(MIN_ZOOM, this.zoomScale - ZOOM_STEP)
  }

  /** 双指缩放：比例低于 MIN_ZOOM 时同样退出当前层回到父星图 */
  private applyPinchScale(scale: number) {
    if (scale < MIN_ZOOM && this.parentStarmapId.length > 0) {
      this.returnToParent()
      return
    }
    this.zoomScale = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, scale))
  }

  /**
   * 导航到子星图（Embed 的目标星图）
   */
  navigateToEmbed(embed: StarMapEmbed) {
    this.navPathStack.pushPath({
      name: Routes.StarMap,
      param: {
        starmapId: embed.targetStarmapId,
        starmapTitle: embed.label ?? embed.targetStarmapId,
        parentStarmapId: this.starmapId,
        parentStarmapTitle: this.starmapTitle
      } as StarMapRouteParams
    })
  }

  onTransformChange() {
    this.drawEdges()
  }

  /**
   * motionProgress 变化时重画边和预览线。
   * wobble 偏移随 progress 变化，如果 Canvas 不重画，节点在晃但线端点不跟。
   */
  onMotionProgressChange() {
    this.drawEdges()
    if (this.gestureMode === 'connect') {
      this.drawPreviewLine()
    }
  }

  // —— 分享生命周期（保留不变）——

  buildSharePayload(): SharePayload {
    const title: string = this.starmapTitle
    let text: string = ''
    if (this.graph !== null) {
      const nodeCount: number = this.graph.nodes.length
      const edgeCount: number = this.graph.edges.length
      const nodeNames: string[] = []
      for (let i = 0; i < this.graph.nodes.length; i++) {
        const node: StarMapNode = this.graph.nodes[i]
        if (node.title.length > 0) {
          nodeNames.push(node.title)
          if (nodeNames.length >= 20) {
            break
          }
        }
      }
      text = `\u661f\u56fe\uff1a${title}\n\u8282\u70b9 ${nodeCount} / \u5173\u7cfb ${edgeCount}`
      if (nodeNames.length > 0) {
        text = text + '\n' + nodeNames.join('\u3001')
      }
    } else {
      if (title.length > 0) {
        text = `\u661f\u56fe\uff1a${title}`
      }
    }
    const payload: SharePayload = {
      kind: 'text',
      text: text
    }
    if (title.length > 0) {
      payload.title = title
    }
    return payload
  }

  async startShareLifecycle(): Promise<void> {
    const payload: SharePayload = this.buildSharePayload()
    try {
      this.shareService.startTapSharing(payload)
    } catch (e) {
    }
    try {
      await this.shareService.startAirTransferSharing(payload)
    } catch (e) {
    }
  }

  updateSharePayload(): void {
    const payload: SharePayload = this.buildSharePayload()
    try {
      this.shareService.updateTapPayload(payload)
      this.shareService.updateAirTransferPayload(payload)
    } catch (e) {
    }
  }

  async stopShareLifecycle(): Promise<void> {
    try {
      this.shareService.stopTapSharing()
    } catch (e) {
    }
    try {
      await this.shareService.stopAirTransferSharing()
    } catch (e) {
    }
    try {
      this.shareService.updateTapPayload(null)
      this.shareService.updateAirTransferPayload(null)
    } catch (e) {
    }
  }

  // —— 星图工具条 ——

  @Builder
  StarMapToolbar() {
    Row() {
      // 返回父星图按钮（仅当有父星图时显示）
      if (this.parentStarmapId.length > 0) {
        Button($r('app.string.back_to_parent'))
          .fontSize(12)
          .height(36)
          .onClick(() => {
            this.returnToParent()
          })
      }

      Button('−')
        .width(36)
        .height(36)
        .fontSize(18)
        .onClick(() => {
          this.zoomOut()
        })

      Text(`${Math.round(this.zoomScale * 100)}%`)
        .fontSize(14)
        .width(60)
        .textAlign(TextAlign.Center)

      Button('+')
        .width(36)
        .height(36)
        .fontSize(18)
        .onClick(() => {
          this.zoomIn()
        })

      Button($r('app.string.reset'))
        .fontSize(12)
        .height(36)
        .onClick(() => {
          this.zoomScale = 1
          this.offsetX = 0
          this.offsetY = 0
        })

      Button($r('app.string.auto_layout'))
        .fontSize(12)
        .height(36)
        .onClick(() => {
          this.applyAutoLayout()
        })

      Button($r('app.string.share'))
        .fontSize(12)
        .height(36)
        .onClick(() => {
          void this.shareService.share(this.buildSharePayload(), 'system')
        })
    }
    .width('100%')
    .padding({ left: 12, right: 12, top: 8, bottom: 8 })
    .justifyContent(FlexAlign.SpaceEvenly)
  }

  @Builder
  StarMapContent() {
    Column() {
      this.StarMapToolbar()

      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
          Text($r('app.string.loading'))
            .fontSize(14)
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .margin({ bottom: 16 })

          Button($r('app.string.retry'))
            .onClick(() => this.loadStarMap())
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.graph) {
        Stack() {
          // Canvas for edges（本地几何计算）
          Canvas(this.buildCanvasContext())
            .width('100%')
            .height('100%')
            .onReady(() => {
              this.drawEdges()
            })

          // 节点层：每个节点用 GestureGroup(LongPress + Pan) 绑定触屏交互
          ForEach(this.graph.nodes, (node: StarMapNode) => {
            this.StarMapNodeItem(node)
          }, (node: StarMapNode) => node.id)

          // Embed 层：渲染子星图嵌入
          ForEach(this.graph.embeds, (embed: StarMapEmbed) => {
            this.StarMapEmbedItem(embed)
          }, (embed: StarMapEmbed) => embed.instanceId)

          // 预览连线（connect 模式下显示）
          if (this.gestureMode === 'connect') {
            Canvas(this.buildPreviewCanvasContext())
              .width('100%')
              .height('100%')
              .onReady(() => {
                this.drawPreviewLine()
              })
          }

          // 节点菜单（nodeMenu 模式下显示）
          if (this.showNodeMenu) {
            this.NodeMenu()
          }

          // Node info panel
          if (this.selectedNode) {
            this.NodeInfoPanel()
          }

          // Legend
          this.Legend()

          // Graph info
          this.GraphInfo()
        }
        .layoutWeight(1)
        .width('100%')
        .onAreaChange((_oldArea: Area, newArea: Area) => {
          this.stackWidth = Number(newArea.width)
          this.stackHeight = Number(newArea.height)
        })
        // 空白区域：PanGesture → 拖动画布（panCanvas 模式）；PinchGesture → 双指缩放
        .gesture(
          GestureGroup(GestureMode.Parallel,
            PinchGesture({ fingers: 2 })
              .onActionStart(() => {
                this.pinchBaseScale = this.zoomScale
              })
              .onActionUpdate((event: GestureEvent) => {
                this.applyPinchScale(this.pinchBaseScale * event.scale)
              }),
            PanGesture()
              .onActionStart((event: GestureEvent) => {
                // 空白区域手势挂在 Stack 上：组件左上角即 Stack 原点
                const point = this.gestureScreenPoint(event, 0, 0)
                this.gestureTracker.beginPanCanvas(point.x, point.y)
                this.gestureMode = 'panCanvas'
                this.panBaseX = this.offsetX
                this.panBaseY = this.offsetY
              })
              .onActionUpdate((event: GestureEvent) => {
                if (this.gestureMode === 'panCanvas') {
                  this.offsetX = this.panBaseX + event.offsetX
                  this.offsetY = this.panBaseY + event.offsetY
                }
              })
              .onActionEnd(() => {
                this.gestureTracker.reset()
                this.gestureMode = 'idle'
              })
          )
        )
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
  }

  build() {
    if (this.useHdsNav) {
      HdsAppDestinationApi23({
        title: this.starmapTitle,
        screenRole: ScreenRole.StarMap,
        screenTabIndex: 1,
        isRoot: false,
        onBack: () => {
          this.navPathStack.pop()
          return true
        }
      }) {
        this.StarMapContent()
      }
    } else {
      NavDestination() {
        this.StarMapContent()
      }
      .hideTitleBar(false)
      .hideBackButton(false)
      .onBackPressed(() => {
        this.navPathStack.pop()
        return true
      })
      .title(this.starmapTitle)
      .menus(buildNavMenuItems(this.headerActions))
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.StarMap, 1)
      })
    }
  }

  /**
   * 节点组件：用 GestureGroup(Parallel, LongPress + Pan) 绑定触屏交互。
   *
   * 触屏规则：
   * - 点击节点 → 选中（只改变视觉选中态，不进入手势模式）
   * - 长按节点 → 弹出节点菜单（nodeMenu 模式）
   * - 长按后继续移动 → 从 nodeMenu 切到 connect，关闭菜单并显示预览线
   * - 松手落到另一节点 → 创建关系
   * - "移动节点"必须从菜单显式进入 moveNode 模式，之后 Pan 才改 node.position
   */
  @Builder
  StarMapNodeItem(node: StarMapNode) {
    Column() {
      Circle({ width: 56 * this.zoomScale, height: 56 * this.zoomScale })
        .fill(this.getNodeColor(node))
        .stroke(this.selectedNodeId === node.id ? '#FF6B6B' : 'transparent')
        .strokeWidth(4)
        .shadow({
          radius: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeNodeId === node.id ?
            DesignTokens.shadowStrong.radius :
            (this.selectedNodeId === node.id ? DesignTokens.shadowMedium.radius : DesignTokens.shadowLight.radius),
          color: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeNodeId === node.id ?
            '#40000000' :
            (this.selectedNodeId === node.id ? '#40FF6B6B' : '#20000000'),
          offsetX: 0,
          offsetY: this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeNodeId === node.id ?
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
      this.gestureTracker.isDraggingNode() && this.gestureTracker.getState().activeNodeId === node.id
        ? { x: this.motionConfig.dragLiftScale, y: this.motionConfig.dragLiftScale }
        : { x: 1, y: 1 }
    )
    .onClick(() => {
      // 点击节点 → 选中（不进入手势模式）
      this.selectNode(node.id)
    })
    .gesture(
      GestureGroup(GestureMode.Parallel,
        // 长按 → 弹出节点菜单
        LongPressGesture({ repeat: false, duration: 500 })
          .onAction((event: GestureEvent) => {
            const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
            this.gestureTracker.beginNodeMenu(node.id, point.x, point.y)
            this.gestureMode = 'nodeMenu'
            this.showNodeMenu = true
            this.nodeMenuNodeId = node.id
            this.selectNode(node.id)
          }),
        // Pan → 根据当前手势模式处理
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'nodeMenu') {
              // 长按后继续移动 → 从 nodeMenu 切到 connect
              const point = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              this.gestureTracker.beginConnect(node.id, point.x, point.y)
              this.gestureMode = 'connect'
              this.showNodeMenu = false
              // 设置预览线起点（节点中心屏幕坐标）
              const ln = this.getPlatformLayoutNode(node.id)
              if (ln) {
                this.previewLineFrom = {
                  x: (ln.x + ln.width / 2) * this.zoomScale + this.offsetX,
                  y: (ln.y + ln.height / 2) * this.zoomScale + this.offsetY
                }
              }
              this.previewLineTo = point
            } else if (state.mode === 'moveNode' && state.activeNodeId === node.id) {
              // 从菜单显式进入 moveNode 后，Pan 改 node.position
              this.motionPauseTracker.pauseNode(node.id)
              const ln = this.getPlatformLayoutNode(node.id)
              if (ln) {
                this.moveNodeStartX = ln.x
                this.moveNodeStartY = ln.y
              }
            }
          })
          .onActionUpdate((event: GestureEvent) => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'connect') {
              // connect 模式：更新预览线终点（触点相对节点的偏移 + 节点屏幕位置）
              this.previewLineTo = this.gestureScreenPoint(event, this.getNodeScreenX(node.id), this.getNodeScreenY(node.id))
              // 预览线跟手重画：onReady 只画一次，手指移动时必须主动重绘
              this.drawPreviewLine()
            } else if (state.mode === 'moveNode' && state.activeNodeId === node.id) {
              // moveNode 模式：更新节点位置（base+offset 避免重复累加）
              const newDx = this.moveNodeStartX + event.offsetX / this.zoomScale
              const newDy = this.moveNodeStartY + event.offsetY / this.zoomScale
              const positions = new Map<string, PlatformPoint>()
              positions.set(node.id, { x: newDx, y: newDy })
              this.platformLayoutNodes = applyFreeformLayout(this.platformLayoutNodes, positions)
              this.recomputeGeometry()
            }
          })
          .onActionEnd(() => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'connect') {
              // connect 模式：检查是否落到另一节点，创建关系
              const screenX = this.previewLineTo.x
              const screenY = this.previewLineTo.y
              // 将屏幕坐标转为画布坐标进行命中测试
              const canvasPoint = screenToCanvas(screenX, screenY, this.zoomScale, this.offsetX, this.offsetY)
              // 建边命中只查普通节点 rect，不能把 Embed 当 Node target
              // （Embed 的 Pan 没有 connect 路径，addStarMapEdge 也只接受 Node ID）
              const nodeOnlyRects: PlatformNodeRect[] = this.nodeRects.filter((r: PlatformNodeRect) => !this.isEmbed(r.nodeId))
              const targetNodeId = hitTest(nodeOnlyRects, canvasPoint.x, canvasPoint.y)
              if (targetNodeId !== null && targetNodeId !== node.id) {
                // 创建关系：走 Core 的 add_starmap_edge
                void this.createEdge(node.id, targetNodeId)
              }
            } else if (state.mode === 'moveNode' && state.activeNodeId === node.id) {
              // 移动结束：通过 bridge 的节点更新 API 把 position 写回 Core
              void this.saveNodePosition(node.id)
              this.motionPauseTracker.resumeNode(node.id)
            }
            // 重置手势状态
            this.gestureTracker.reset()
            this.gestureMode = 'idle'
            this.showNodeMenu = false
            this.nodeMenuNodeId = ''
          })
      )
    )
  }

  /**
   * Embed 组件：渲染子星图嵌入，支持点击进入子星图和长按弹出菜单。
   */
  @Builder
  StarMapEmbedItem(embed: StarMapEmbed) {
    Column() {
      Text(embed.label ?? embed.targetStarmapId)
        .fontSize(14 * this.zoomScale)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 4 * this.zoomScale })

      Text($r('app.string.embed_subtitle'))
        .fontSize(12 * this.zoomScale)
        .fontColor('#666666')
    }
    .width(200 * this.zoomScale)
    .height(120 * this.zoomScale)
    .borderRadius(12 * this.zoomScale)
    .backgroundColor('#E8E8E8')
    .border({ width: 2, color: '#CCCCCC' })
    .position({
      x: this.getEmbedScreenX(embed.instanceId),
      y: this.getEmbedScreenY(embed.instanceId)
    })
    .onClick(() => {
      // 点击 Embed → 进入子星图
      this.navigateToEmbed(embed)
    })
    .gesture(
      GestureGroup(GestureMode.Parallel,
        LongPressGesture({ repeat: false, duration: 500 })
          .onAction((event: GestureEvent) => {
            const point = this.gestureScreenPoint(event, this.getEmbedScreenX(embed.instanceId), this.getEmbedScreenY(embed.instanceId))
            this.gestureTracker.beginNodeMenu(embed.instanceId, point.x, point.y)
            this.gestureMode = 'nodeMenu'
            this.showNodeMenu = true
            this.nodeMenuNodeId = embed.instanceId
          }),
        PanGesture()
          .onActionStart((event: GestureEvent) => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveEmbed' && state.activeNodeId === embed.instanceId) {
              this.motionPauseTracker.pauseNode(embed.instanceId)
              const ln = this.getPlatformLayoutNode(embed.instanceId)
              if (ln) {
                this.moveEmbedStartX = ln.x
                this.moveEmbedStartY = ln.y
              }
            }
          })
          .onActionUpdate((event: GestureEvent) => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveEmbed' && state.activeNodeId === embed.instanceId) {
              // moveEmbed 模式：更新 Embed 位置（base+offset 避免重复累加）
              const newDx = this.moveEmbedStartX + event.offsetX / this.zoomScale
              const newDy = this.moveEmbedStartY + event.offsetY / this.zoomScale
              const positions = new Map<string, PlatformPoint>()
              positions.set(embed.instanceId, { x: newDx, y: newDy })
              this.embedLayoutNodes = applyFreeformLayout(this.embedLayoutNodes, positions)
              this.recomputeGeometry()
            }
          })
          .onActionEnd(() => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'moveEmbed') {
              // 移动结束：通过 bridge 的 Embed update API 把 position 写回 Core
              void this.saveEmbedPosition(embed.instanceId)
              this.motionPauseTracker.resumeNode(embed.instanceId)
            }
            this.gestureTracker.reset()
            this.gestureMode = 'idle'
            this.showNodeMenu = false
            this.nodeMenuNodeId = ''
          })
      )
    )
  }

  /**
   * 创建关系（边）：通过 bridge 的星图 CRUD API。
   */
  async createEdge(fromNodeId: string, toNodeId: string) {
    hilog.info(0x0000, 'StarMapScreen', `createEdge: ${fromNodeId} → ${toNodeId}`)
    const result = await this.bridge.addStarMapEdge(this.starmapId, fromNodeId, toNodeId, 'RelatedTo', null)
    if (result.success) {
      // 重新加载星图以获取新边
      this.loadStarMap()
    } else {
      hilog.error(0x0000, 'StarMapScreen', `createEdge failed: ${result.errorCode}, ${result.userMessage}`)
    }
  }

  /**
   * 保存节点位置：通过 bridge 的星图节点更新 API 写回。
   */
  async saveNodePosition(nodeId: string) {
    const ln = this.getPlatformLayoutNode(nodeId)
    if (!ln) return
    hilog.info(0x0000, 'StarMapScreen', `saveNodePosition: ${nodeId} → (${ln.x}, ${ln.y})`)
    const result = await this.bridge.updateStarMapNode(this.starmapId, nodeId, { x: ln.x, y: ln.y })
    if (!result.success) {
      hilog.error(0x0000, 'StarMapScreen', `saveNodePosition failed: ${result.errorCode ?? ''}, ${result.userMessage}`)
    }
  }

  /**
   * 节点菜单：长按节点/Embed后弹出。
   * 普通节点和 Embed 分开菜单：Embed 只有"移动子星图"，普通节点有"移动节点"和"创建关系"。
   * Embed 上"移动节点"/"创建关系"会进入无效状态（Embed 的 Pan 不处理 moveNode/connect）。
   */
  @Builder
  NodeMenu() {
    Column() {
      Text(this.getPlatformLayoutNode(this.nodeMenuNodeId)?.nodeId ?? '')
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 8 })

      if (this.isEmbed(this.nodeMenuNodeId)) {
        // Embed 菜单：只放 Embed 自己的动作
        Button($r('app.string.move_embed'))
          .fontSize(13)
          .height(36)
          .width('100%')
          .onClick(() => {
            this.gestureTracker.beginMoveEmbed(this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveEmbed'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })
      } else {
        // 普通节点菜单：移动节点 + 创建关系
        Button($r('app.string.move_node'))
          .fontSize(13)
          .height(36)
          .width('100%')
          .onClick(() => {
            this.gestureTracker.beginMoveNode(this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'moveNode'
            this.showNodeMenu = false
            this.motionPauseTracker.pauseNode(this.nodeMenuNodeId)
          })

        Button($r('app.string.connect_node'))
          .fontSize(13)
          .height(36)
          .width('100%')
          .margin({ top: 4 })
          .onClick(() => {
            this.gestureTracker.beginConnect(this.nodeMenuNodeId, 0, 0)
            this.gestureMode = 'connect'
            this.showNodeMenu = false
            const ln = this.getPlatformLayoutNode(this.nodeMenuNodeId)
            if (ln) {
              this.previewLineFrom = {
                x: (ln.x + ln.width / 2) * this.zoomScale + this.offsetX,
                y: (ln.y + ln.height / 2) * this.zoomScale + this.offsetY
              }
            }
            this.previewLineTo = this.previewLineFrom
          })
      }

      // 取消菜单（两种情况都有）
      Button($r('app.string.cancel'))
        .fontSize(13)
        .height(36)
        .width('100%')
        .margin({ top: 4 })
        .onClick(() => {
          this.gestureTracker.reset()
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

  @Builder
  NodeInfoPanel() {
    Column() {
      Row() {
        Text($r('app.string.node_info'))
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
        Blank()

        Text('✕')
          .fontSize(18)
          .onClick(() => {
            this.deselectNode()
          })
      }
      .width('100%')
      .margin({ bottom: 12 })

      Column() {
        Row() {
          Circle({ width: 12, height: 12 })
            .fill(this.getNodeColor(this.selectedNode!))
            .margin({ right: 8 })

          Text(this.getNodeTypeName(this.selectedNode!.kind))
            .fontSize(12)
        }
        .margin({ bottom: 12 })

        Text(this.selectedNode!.title)
          .fontSize(22)
          .fontWeight(FontWeight.Bold)
          .margin({ bottom: 8 })

        if (this.selectedNode!.content.summary) {
          Text(this.selectedNode!.content.summary)
            .fontSize(14)
            .margin({ bottom: 12 })
        }

        // 节点坐标 — 从 platformLayoutNodes 查找
        Row() {
          Text(`X: ${Math.round(this.getPlatformLayoutNode(this.selectedNodeId)?.x ?? 0)}`)
            .fontSize(12)
            .margin({ right: 16 })

          Text(`Y: ${Math.round(this.getPlatformLayoutNode(this.selectedNodeId)?.y ?? 0)}`)
            .fontSize(12)
        }
        .margin({ bottom: 16 })

        if (this.graph) {
          Text($r('app.string.related_relations'))
            .fontSize(14)
            .fontWeight(FontWeight.Medium)
            .margin({ bottom: 8 })

          ForEach(this.graph.edges.filter(e =>
            e.from.target.nodeId === this.selectedNodeId || e.to.target.nodeId === this.selectedNodeId
          ), (edge: StarMapEdge) => {
            Row() {
              Circle({ width: 8, height: 8 })
                .margin({ right: 8 })

              Column() {
                Text(edge.label || getContext(this).resourceManager.getStringSync($r('app.string.unnamed_relation').id))
                  .fontSize(13)
                Text(this.getEdgeTypeName(edge.kind))
                  .fontSize(11)
              }
              .layoutWeight(1)

              Text(edge.from.target.nodeId === this.selectedNodeId ? '→' : '←')
                .fontSize(14)
            }
            .margin({ bottom: 8 })
          }, (edge: StarMapEdge) => edge.id)
        }
      }
    }
    .width(300)
    .padding(16)
    .borderRadius(DesignTokens.radiusMd)
    .shadow({ radius: DesignTokens.shadowMedium.radius, offsetX: 0, offsetY: DesignTokens.shadowMedium.offsetY })
    .position({
      x: this.adaptiveState.sizeClass === WidthClass.Expanded ? this.stackWidth - 300 - 16 : 16,
      y: 16
    })
  }

  @Builder
  Legend() {
    Column() {
      Text($r('app.string.legend'))
        .fontSize(12)
        .margin({ bottom: 8 })

      Row() {
        this.LegendItem('#4A90D9', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_character').id))
        this.LegendItem('#F39C12', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_location').id))
        this.LegendItem('#9B59B6', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_event').id))
        this.LegendItem('#2ECC71', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_theme').id))
      }
      .margin({ bottom: 4 })

      Row() {
        this.LegendItem('#E74C3C', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_plot').id))
        this.LegendItem('#1ABC9C', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_item').id))
        this.LegendItem('#3498DB', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_organization').id))
        this.LegendItem('#95A5A6', getContext(this).resourceManager.getStringSync($r('app.string.node_kind_custom').id))
      }
    }
    .padding(12)
    .borderRadius(DesignTokens.radiusSm)
    .shadow({ radius: DesignTokens.shadowLight.radius, offsetX: 0, offsetY: DesignTokens.shadowLight.offsetY })
    .position({
      x: this.adaptiveState.sizeClass === WidthClass.Expanded ? this.stackWidth - 200 - 16 : 16,
      y: this.stackHeight - 120 - 16
    })
  }

  @Builder
  LegendItem(color: string, label: string) {
    Row() {
      Circle({ width: 10, height: 10 })
        .fill(color)
        .margin({ right: 4 })

      Text(label)
        .fontSize(11)
    }
    .margin({ right: 12 })
  }

  @Builder
  GraphInfo() {
    Column() {
      if (this.graph) {
        Text(getContext(this).resourceManager.getStringSync($r('app.string.nodes_edges_format').id).replace('%s', `${this.graph.nodes.length}`).replace('%s', `${this.graph.edges.length}`))
          .fontSize(12)
      }
    }
    .padding(8)
    .borderRadius(DesignTokens.radiusXs)
    .shadow({ radius: 2, offsetX: 0, offsetY: 1 })
    .position({ x: this.stackWidth - 160 - 16, y: this.stackHeight - 40 - 16 })
  }

  // —— Canvas 绘制 ——

  private canvasCtx: CanvasRenderingContext2D | null = null
  private previewCanvasCtx: CanvasRenderingContext2D | null = null

  buildCanvasContext(): CanvasRenderingContext2D {
    this.canvasCtx = new CanvasRenderingContext2D(new RenderingContextSettings(true))
    return this.canvasCtx
  }

  buildPreviewCanvasContext(): CanvasRenderingContext2D {
    this.previewCanvasCtx = new CanvasRenderingContext2D(new RenderingContextSettings(true))
    return this.previewCanvasCtx
  }

  /**
   * 绘制边：使用本地 localEdgeRenders（不再从 Core 获取）。
   * wobble 偏移在节点位置计算中已包含，此处只需应用 zoom/offset 变换。
   */
  drawEdges() {
    if (!this.canvasCtx || !this.graph) return
    const ctx = this.canvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    // 构建 nodeId → wobble offset 映射
    const progress = this.motionProgress
    const wobbleMap = new Map<string, WobbleOffset>()
    for (const node of this.graph.nodes) {
      const dx = wobbleXFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeNodeId)
      const dy = wobbleYFromProgress(node.id, progress, this.motionConfig, this.gestureTracker.getState().activeNodeId)
      wobbleMap.set(node.id, { dx, dy })
    }

    for (const edge of this.localEdgeRenders) {
      const graphEdge = this.graph.edges.find(e => e.id === edge.edgeId)
      if (!graphEdge) continue

      const fromId = graphEdge.from.target.nodeId || ''
      const toId = graphEdge.to.target.nodeId || ''
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
      ctx.strokeStyle = isSelected ? "#007DFF" : "#33000000"
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
      ctx.strokeStyle = isSelected ? "#007DFF" : "#33000000"
      ctx.lineWidth = isSelected ? 2.5 : 1.5
      ctx.stroke()

      // 边标签
      if (graphEdge.label && edge.labelX !== undefined && edge.labelY !== undefined) {
        const lx = (edge.labelX + (fromWobble.dx + toWobble.dx) / 2) * this.zoomScale + this.offsetX
        const ly = (edge.labelY + (fromWobble.dy + toWobble.dy) / 2) * this.zoomScale + this.offsetY
        ctx.font = '11px HarmonyOS Sans'
        ctx.fillStyle = isSelected ? "#000000" : "#99000000"
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
    if (!this.previewCanvasCtx) return
    const ctx = this.previewCanvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    ctx.beginPath()
    ctx.moveTo(this.previewLineFrom.x, this.previewLineFrom.y)
    ctx.lineTo(this.previewLineTo.x, this.previewLineTo.y)
    ctx.strokeStyle = "#007DFF"
    ctx.lineWidth = 2
    ctx.setLineDash([8, 4])
    ctx.stroke()
    ctx.setLineDash([])
  }
}
