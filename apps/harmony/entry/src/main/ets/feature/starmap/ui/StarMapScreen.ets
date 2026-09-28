import { Routes, StarMapRouteParams } from '../../../app/navigation/Routes'
import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { StarMapGraph, StarMapNode, StarMapEdge, StarMapMeta } from '../../../corebridge/dto/StarMapDtos'
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
import { PlatformNodeRect, PlatformEdgeRender, deriveNodeRects, hitTest, computeAllEdgeRenders, canvasToScreen, screenToCanvas } from '../platform/StarMapGeometry'
import { PlatformLayoutNode, NodePositionPatch, defaultLayoutNodes, applyFreeformLayout, generatePositionPatches } from '../platform/StarMapLayout'
import { PlatformMotionConfig, defaultMotionConfig, stablePhase, wobbleX, wobbleY, MotionPauseTracker, shouldAnimate } from '../platform/StarMapMotion'
import { GestureMode, GestureState, StarMapGestureStateTracker } from '../platform/StarMapGestureState'

interface SelectOption {
  value: string
  icon: string
}

@Component
export struct StarMapScreen {
  @Consume("navPathStack") navPathStack: NavPathStack
  @State starmapId: string = ''
  @State starmapTitle: string = ''
  @State graph: StarMapGraph | null = null
  @State allStarMaps: StarMapMeta[] = []
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
  @State nodeRects: PlatformNodeRect[] = []
  @State localEdgeRenders: PlatformEdgeRender[] = []

  // 运动配置（本地默认值，不从 Core 获取）
  @State motionConfig: PlatformMotionConfig = defaultMotionConfig()
  private motionPauseTracker: MotionPauseTracker = new MotionPauseTracker()

  // 手势状态
  private gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  @State gestureMode: GestureMode = 'idle'
  @State showNodeMenu: boolean = false
  @State nodeMenuNodeId: string = ''
  @State previewLineFrom: { x: number, y: number } = { x: 0, y: 0 }
  @State previewLineTo: { x: number, y: number } = { x: 0, y: 0 }

  // 动画驱动：用 animateTo 驱动 animPhase 从 0→1 无限循环，替代 setInterval
  @State animPhase: number = 0

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
      this.loadStarMap()
      this.loadAllStarMaps()
    } else {
      this.loadAllStarMapsAndSelectFirst()
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

    // 启动 wobble 动画：用 animateTo 驱动 animPhase 从 0→1 无限循环
    this.startWobbleAnimation()

    void this.startShareLifecycle()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    void this.stopShareLifecycle()
  }

  /**
   * 用 animateTo 驱动 animPhase 从 0→1 无限循环，替代 setInterval 定时器。
   * wobbleX/wobbleY 用 animPhase * idlePeriodMs 作为 tick 计算偏移。
   */
  startWobbleAnimation() {
    if (!shouldAnimate(this.motionConfig)) {
      return
    }
    animateTo({
      duration: this.motionConfig.idlePeriodMs,
      iterations: -1,
      curve: Curve.Linear,
      onFinish: () => {
        this.animPhase = this.animPhase + 1
      }
    }, () => {
      this.animPhase = this.animPhase + 1
    })
  }

  async loadStarMap() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.getStarMapGraph(this.starmapId)
      if (result.success && result.data) {
        this.graph = result.data
        // 使用平台层布局：从 graph.nodes 的 ID 列表创建默认布局节点
        const nodeIds: string[] = result.data.nodes.map((node: StarMapNode) => node.id)
        this.platformLayoutNodes = defaultLayoutNodes(nodeIds)
        // 应用自动网格布局（居中排布）
        if (this.stackWidth > 0 && this.stackHeight > 0) {
          this.applyAutoLayout()
        }
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
   * 自动网格布局：将节点按行列排布并居中。
   */
  applyAutoLayout() {
    if (this.platformLayoutNodes.length === 0 || this.stackWidth === 0) {
      return
    }
    const nodeCount = this.platformLayoutNodes.length
    const cols = Math.ceil(Math.sqrt(nodeCount))
    const rows = Math.ceil(nodeCount / cols)
    const gridHSpacing = 200
    const gridVSpacing = 120
    const gridWidth = (cols - 1) * gridHSpacing + 160
    const gridHeight = (rows - 1) * gridVSpacing + 80
    const offsetX = (this.stackWidth - gridWidth) / 2
    const offsetY = (this.stackHeight - gridHeight) / 2

    const positions = new Map<string, { x: number, y: number }>()
    for (let i = 0; i < this.platformLayoutNodes.length; i++) {
      const col = i % cols
      const row = Math.floor(i / cols)
      positions.set(this.platformLayoutNodes[i].nodeId, {
        x: offsetX + col * gridHSpacing,
        y: offsetY + row * gridVSpacing
      })
    }
    this.platformLayoutNodes = applyFreeformLayout(this.platformLayoutNodes, positions)
    this.recomputeGeometry()
  }

  /**
   * 重新计算所有几何：nodeRects + localEdgeRenders。
   * 在节点位置变化、graph 加载后调用。
   */
  recomputeGeometry() {
    if (!this.graph) {
      this.nodeRects = []
      this.localEdgeRenders = []
      return
    }
    // 从 platformLayoutNodes 派生 nodeRects
    this.nodeRects = this.platformLayoutNodes.map((ln: PlatformLayoutNode): PlatformNodeRect => {
      return {
        nodeId: ln.nodeId,
        x: ln.x,
        y: ln.y,
        width: ln.width,
        height: ln.height,
        radius: ln.radius
      }
    })
    // 构建 nodeId → PlatformNodeRect 映射
    const rectMap = new Map<string, PlatformNodeRect>()
    for (const rect of this.nodeRects) {
      rectMap.set(rect.nodeId, rect)
    }
    // 本地计算 edgeRenders
    this.localEdgeRenders = computeAllEdgeRenders(this.graph.edges, rectMap)
  }

  async loadAllStarMaps() {
    try {
      const result = await this.bridge.listStarMaps()
      if (result.success && result.data) {
        this.allStarMaps = result.data
      }
    } catch (err) {
      // Ignore
    }
  }

  async loadAllStarMapsAndSelectFirst() {
    try {
      const result = await this.bridge.listStarMaps()
      if (result.success && result.data && result.data.length > 0) {
        this.allStarMaps = result.data
        const first = result.data[0]
        this.starmapId = first.starmapId
        this.starmapTitle = first.title
        this.loadStarMap()
      } else {
        this.isLoading = false
      }
    } catch (err) {
      this.isLoading = false
    }
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
   * 获取当前动画 tick 值：animPhase * idlePeriodMs
   */
  private getAnimTick(): number {
    return this.animPhase * this.motionConfig.idlePeriodMs
  }

  /**
   * 获取平台层布局节点
   */
  private getPlatformLayoutNode(nodeId: string): PlatformLayoutNode | undefined {
    return this.platformLayoutNodes.find(ln => ln.nodeId === nodeId)
  }

  /**
   * 计算节点在屏幕上的最终 x 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenX(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseX = ln ? ln.x : 0
    const wobbleDx = wobbleX(nodeId, this.getAnimTick(), this.motionConfig, this.gestureTracker.getState().activeNodeId)
    return (baseX + wobbleDx) * this.zoomScale + this.offsetX - (ln ? ln.width : 160) / 2
  }

  /**
   * 计算节点在屏幕上的最终 y 坐标（含 zoom、offset、wobble）
   */
  private getNodeScreenY(nodeId: string): number {
    const ln = this.getPlatformLayoutNode(nodeId)
    const baseY = ln ? ln.y : 0
    const wobbleDy = wobbleY(nodeId, this.getAnimTick(), this.motionConfig, this.gestureTracker.getState().activeNodeId)
    return (baseY + wobbleDy) * this.zoomScale + this.offsetY - (ln ? ln.height : 80) / 2
  }

  onTransformChange() {
    this.drawEdges()
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

  // —— 星图工具条（保留不变）——

  @Builder
  StarMapToolbar() {
    Row() {
      Select(this.allStarMaps.map((sm: StarMapMeta): SelectOption => {
        return { value: sm.title, icon: '' } as SelectOption
      }))
        .selected(this.allStarMaps.findIndex((sm: StarMapMeta) => sm.starmapId === this.starmapId))
        .value(this.starmapTitle)
        .onSelect((index: number) => {
          if (index >= 0 && index < this.allStarMaps.length) {
            this.starmapId = this.allStarMaps[index].starmapId
            this.starmapTitle = this.allStarMaps[index].title
            this.loadStarMap()
          }
        })
        .width(160)

      Button('−')
        .width(36)
        .height(36)
        .fontSize(18)
        .onClick(() => {
          this.zoomScale = Math.max(0.3, this.zoomScale - 0.1)
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
          this.zoomScale = Math.min(3, this.zoomScale + 0.1)
        })

      Button($r('app.string.reset'))
        .fontSize(12)
        .height(36)
        .onClick(() => {
          this.zoomScale = 1
          this.offsetX = 0
          this.offsetY = 0
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
          // 首次获取尺寸后应用自动布局
          if (this.platformLayoutNodes.length > 0 && this.getPlatformLayoutNode(this.platformLayoutNodes[0].nodeId)?.x === 0) {
            this.applyAutoLayout()
          }
        })
        // 空白区域：PanGesture → 拖动画布（panCanvas 模式）
        .gesture(
          PanGesture()
            .onActionStart((event: GestureEvent) => {
              this.gestureTracker.beginPanCanvas(event.x, event.y)
              this.gestureMode = 'panCanvas'
            })
            .onActionUpdate((event: GestureEvent) => {
              if (this.gestureMode === 'panCanvas') {
                this.offsetX += event.offsetX
                this.offsetY += event.offsetY
              }
            })
            .onActionEnd(() => {
              this.gestureTracker.reset()
              this.gestureMode = 'idle'
            })
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
        screenTabIndex: 1
      }) {
        this.StarMapContent()
      }
    } else {
      NavDestination() {
        this.StarMapContent()
      }
      .hideTitleBar(false)
      .title(this.starmapTitle)
      .menus(buildNavMenuItems(this.headerActions))
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.StarMap, 1)
      })
    }
  }

  /**
   * 节点组件：用 GestureGroup(Exclusive, LongPress + Pan) 绑定触屏交互。
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
      Circle({ width: 56, height: 56 })
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
        .fontSize(13)
        .fontWeight(this.selectedNodeId === node.id ? FontWeight.Bold : FontWeight.Normal)
        .margin({ top: 6 })
        .maxLines(2)
        .textAlign(TextAlign.Center)
        .width(80)
    }
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
      GestureGroup(GestureMode.Exclusive,
        // 长按 → 弹出节点菜单
        LongPressGesture({ repeat: false, duration: 500 })
          .onAction((event: GestureEvent) => {
            this.gestureTracker.beginNodeMenu(node.id, event.x, event.y)
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
              this.gestureTracker.beginConnect(node.id, event.x, event.y)
              this.gestureMode = 'connect'
              this.showNodeMenu = false
              // 设置预览线起点（节点中心屏幕坐标）
              const ln = this.getPlatformLayoutNode(node.id)
              if (ln) {
                this.previewLineFrom = {
                  x: ln.x * this.zoomScale + this.offsetX,
                  y: ln.y * this.zoomScale + this.offsetY
                }
              }
              this.previewLineTo = { x: event.x, y: event.y }
            } else if (state.mode === 'moveNode' && state.activeNodeId === node.id) {
              // 从菜单显式进入 moveNode 后，Pan 改 node.position
              this.motionPauseTracker.pauseNode(node.id)
            }
          })
          .onActionUpdate((event: GestureEvent) => {
            const state = this.gestureTracker.getState()
            if (state.mode === 'connect') {
              // connect 模式：更新预览线终点
              this.previewLineTo = { x: event.x, y: event.y }
            } else if (state.mode === 'moveNode' && state.activeNodeId === node.id) {
              // moveNode 模式：更新节点位置
              const layoutIndex = this.platformLayoutNodes.findIndex(ln => ln.nodeId === node.id)
              if (layoutIndex >= 0) {
                this.platformLayoutNodes[layoutIndex].x += event.offsetX / this.zoomScale
                this.platformLayoutNodes[layoutIndex].y += event.offsetY / this.zoomScale
                this.recomputeGeometry()
              }
            } else if (state.mode === 'moveEmbed' && state.activeNodeId === node.id) {
              // moveEmbed 模式：更新 Embed 位置（与 moveNode 同逻辑）
              const layoutIndex = this.platformLayoutNodes.findIndex(ln => ln.nodeId === node.id)
              if (layoutIndex >= 0) {
                this.platformLayoutNodes[layoutIndex].x += event.offsetX / this.zoomScale
                this.platformLayoutNodes[layoutIndex].y += event.offsetY / this.zoomScale
                this.recomputeGeometry()
              }
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
              const targetNodeId = hitTest(this.nodeRects, canvasPoint.x, canvasPoint.y)
              if (targetNodeId !== null && targetNodeId !== node.id) {
                // 创建关系：通过 bridge 的星图 CRUD API
                this.createEdge(node.id, targetNodeId)
              }
            } else if (state.mode === 'moveNode' || state.mode === 'moveEmbed') {
              // 移动结束：保存节点位置（通过 bridge 的星图节点更新 API）
              this.saveNodePosition(node.id)
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
   * 创建关系（边）：通过 bridge 的星图 CRUD API。
   * 当前 bridge 没有直接的 createEdge 方法，此处预留接口。
   */
  createEdge(fromNodeId: string, toNodeId: string) {
    hilog.info(0x0000, 'StarMapScreen', `createEdge: ${fromNodeId} → ${toNodeId}`)
    // TODO: 通过 bridge 创建边
  }

  /**
   * 保存节点位置：生成 position patch，通过 bridge 的星图节点更新 API 写回。
   * 不再调 saveStarMapLayout。
   */
  saveNodePosition(nodeId: string) {
    const ln = this.getPlatformLayoutNode(nodeId)
    if (!ln) return
    const patches = generatePositionPatches([ln])
    hilog.info(0x0000, 'StarMapScreen', `saveNodePosition: ${nodeId} → (${ln.x}, ${ln.y})`)
    // TODO: 通过 bridge 的节点更新 API 写回 position patch
  }

  /**
   * 节点菜单：长按节点后弹出，提供"移动节点"和"创建关系"选项。
   */
  @Builder
  NodeMenu() {
    Column() {
      Text(this.getPlatformLayoutNode(this.nodeMenuNodeId)?.nodeId ?? '')
        .fontSize(14)
        .fontWeight(FontWeight.Medium)
        .margin({ bottom: 8 })

      // 移动节点：从菜单显式进入 moveNode 模式
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

      // 创建关系：从菜单显式进入 connect 模式
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
              x: ln.x * this.zoomScale + this.offsetX,
              y: ln.y * this.zoomScale + this.offsetY
            }
          }
          this.previewLineTo = this.previewLineFrom
        })

      // 取消菜单
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
    const tick = this.getAnimTick()
    const wobbleMap = new Map<string, { dx: number, dy: number }>()
    for (const node of this.graph.nodes) {
      const dx = wobbleX(node.id, tick, this.motionConfig, this.gestureTracker.getState().activeNodeId)
      const dy = wobbleY(node.id, tick, this.motionConfig, this.gestureTracker.getState().activeNodeId)
      wobbleMap.set(node.id, { dx, dy })
    }

    for (const edge of this.localEdgeRenders) {
      const graphEdge = this.graph.edges.find(e => e.id === edge.edgeId)
      if (!graphEdge) continue

      const fromId = graphEdge.from.target.nodeId || ''
      const toId = graphEdge.to.target.nodeId || ''
      const defaultWobble = { dx: 0, dy: 0 }
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
