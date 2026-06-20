import router from '@ohos.router'
import { IWriterCoreBridge } from '../bridge/IWriterCoreBridge'
import { StarMapGraph, StarMapNode, StarMapEdge, StarMapMeta, StarMapLayoutNode, StarMapEdgeRender, StarMapLayout, StarMapMotionPolicy } from '../model/CoreDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../system/HarmonyThemeAdapter'
import { getBridge } from '../common/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../common/AdaptiveContext'
import { WidthClass } from '../model/CoreDtos'

interface StarMapPageParams {
  starmapId: string
  starmapTitle: string
}

interface SelectOption {
  value: string
  icon: string
}

interface WobbleOffset {
  dx: number
  dy: number
}

@Entry
@Component
struct StarMapPage {
  @State starmapId: string = ''
  @State starmapTitle: string = ''
  @State graph: StarMapGraph | null = null
  @State layoutNodes: StarMapLayoutNode[] = []
  @State edgeRenders: StarMapEdgeRender[] = []
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

  // 动画状态变量
  @State motionTick: number = 0
  @State draggingNodeId: string = ''
  @State motionEnabled: boolean = true
  @State idleWobbleEnabled: boolean = true
  @State idleAmplitudeVp: number = 2.0
  @State idlePeriodMs: number = 4200
  @State dragLiftScale: number = 1.04
  @State settleDurationMs: number = 220
  @State reduceMotion: boolean = false

  // Stack 容器尺寸（用于 position 数值计算）
  @State stackWidth: number = 0
  @State stackHeight: number = 0

  private motionTimer: number = -1

  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null

  aboutToAppear() {
    const params = router.getParams() as StarMapPageParams
    if (params) {
      this.starmapId = params.starmapId
      this.starmapTitle = params.starmapTitle
      this.loadStarMap()
      this.loadAllStarMaps()
    }

    const self = this
    this.themeObserver = new ThemeObserverImpl((theme: ThemeConfig) => {
      self.themeConfig = theme
    })
    this.themeAdapter.addObserver(this.themeObserver!)

    // 注册响应式布局观察者
    this.adaptiveObserver = new SimpleAdaptiveObserver((state: AdaptiveState) => {
      self.adaptiveState = state
    })
    AdaptiveContext.getInstance().addObserver(this.adaptiveObserver)
    // 初始化时同步一次状态
    this.adaptiveState = AdaptiveContext.getInstance().getCurrentState()

    // 加载动画策略
    this.loadMotionPolicy()

    // 启动动画定时器
    this.startMotionTimer()
  }

  aboutToDisappear() {
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
    // 停止动画定时器
    if (this.motionTimer !== -1) {
      clearInterval(this.motionTimer)
      this.motionTimer = -1
    }
  }

  async loadStarMap() {
    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.getStarMapGraph(this.starmapId)
      if (result.success && result.data) {
        this.graph = result.data
        // 从 graph.layout.nodes 提取布局坐标
        if (result.data.layout && result.data.layout.nodes) {
          this.layoutNodes = result.data.layout.nodes
        } else {
          // 兼容：如果 layout.nodes 为空，从旧数据降级
          this.layoutNodes = result.data.nodes.map((node: StarMapNode): StarMapLayoutNode => {
            const legacy = node as Record<string, Object>
            const layoutNode: StarMapLayoutNode = {
              nodeId: node.id,
              x: (legacy['x'] as number) || 0,
              y: (legacy['y'] as number) || 0,
              width: (legacy['width'] as number) || 160,
              height: (legacy['height'] as number) || 80,
              radius: 16,
              collapsed: false,
              zIndex: 0,
              scale: 1,
              depth: 0,
              focusWeight: 0,
              orbitGroup: undefined
            }
            return layoutNode
          })
        }
        // 获取 edgeRenders（Core 计算连线几何）
        const edgeResult = await this.bridge.computeStarMapEdgeRenders(result.data)
        if (edgeResult.success && edgeResult.data) {
          this.edgeRenders = edgeResult.data
        }
      } else {
        this.errorMessage = result.userMessage || '加载星图失败'
      }
    } catch (err) {
      this.errorMessage = '加载星图时发生错误'
    } finally {
      this.isLoading = false
    }
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

  async loadMotionPolicy() {
    try {
      const result = await this.bridge.getStarMapMotionPolicy()
      if (result.success && result.data) {
        const policy = result.data
        this.motionEnabled = policy.enabled
        this.idleWobbleEnabled = policy.idleWobbleEnabled
        this.idleAmplitudeVp = policy.idleAmplitudeVp
        this.idlePeriodMs = policy.idlePeriodMs
        this.dragLiftScale = policy.dragLiftScale
        this.settleDurationMs = policy.settleDurationMs
        this.reduceMotion = policy.reduceMotion
      }
    } catch (err) {
      // 保留本地默认值
    }
  }

  getNodeColor(node: StarMapNode): string {
    if (node.color) return node.color
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
      'Character': '角色',
      'Location': '地点',
      'Event': '事件',
      'Theme': '主题',
      'Plot': '情节',
      'Item': '物品',
      'Organization': '组织',
      'Custom': '自定义'
    }
    return kindNames[kind] || '未知'
  }

  getEdgeTypeName(kind: string): string {
    const kindNames: Record<string, string> = {
      'CharacterRelation': '角色关系',
      'DependsOn': '依赖',
      'Timeline': '时间线',
      'ConflictsWith': '冲突',
      'RelatedTo': '关联',
      'Custom': '自定义'
    }
    return kindNames[kind] || '未知'
  }

  selectNode(nodeId: string) {
    this.selectedNodeId = nodeId
    this.selectedNode = this.graph?.nodes.find(n => n.id === nodeId) || null
  }

  /**
   * 用 nodeId hash 算稳定 phase
   */
  stablePhase(nodeId: string): number {
    let hash = 0
    for (let i = 0; i < nodeId.length; i++) {
      hash = ((hash << 5) - hash) + nodeId.charCodeAt(i)
      hash = hash & hash // Convert to 32bit integer
    }
    return (Math.abs(hash) % 360) * Math.PI / 180
  }

  /**
   * 计算节点的视觉水平偏移（idle wobble），不修改 node.x/y。
   * 使用 nodeId hash 算稳定 phase，不用随机数。
   */
  wobbleX(nodeId: string, tick: number): number {
    if (!this.motionEnabled || !this.idleWobbleEnabled || this.reduceMotion || this.draggingNodeId === nodeId) {
      return 0
    }
    const phase = this.stablePhase(nodeId)
    const period = this.idlePeriodMs
    const amplitude = this.idleAmplitudeVp
    return Math.sin((tick / period) * 2 * Math.PI + phase) * amplitude
  }

  /**
   * 计算节点的视觉垂直偏移（idle wobble），不修改 node.y。
   * Y 方向频率略低（0.7x）、振幅更小（0.6x），营造自然漂浮感。
   */
  wobbleY(nodeId: string, tick: number): number {
    if (!this.motionEnabled || !this.idleWobbleEnabled || this.reduceMotion || this.draggingNodeId === nodeId) {
      return 0
    }
    const phase = this.stablePhase(nodeId)
    const period = this.idlePeriodMs
    const amplitude = this.idleAmplitudeVp
    return Math.cos((tick / period) * 0.7 * 2 * Math.PI + phase) * amplitude * 0.6
  }

  /**
   * 启动动画定时器，约 33ms 一帧（~30fps），wobble 不需要高频率。
   */
  startMotionTimer() {
    if (this.motionTimer !== -1) {
      clearInterval(this.motionTimer)
    }
    this.motionTimer = setInterval(() => {
      if (this.motionEnabled && !this.reduceMotion) {
        this.motionTick = Date.now()
      }
    }, 33)
  }

  deselectNode() {
    this.selectedNodeId = ''
    this.selectedNode = null
  }

  onTransformChange() {
    this.drawEdges()
  }

  build() {
    Column() {
      // Header
      Row() {
        Image($r('sys.media.ohos_ic_public_arrow_left'))
          .width(24)
          .height(24)
          .fillColor(this.themeConfig.colors.textPrimary)
          .onClick(() => router.back())

        Text(this.starmapTitle)
          .fontSize(20)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ left: 12 })

        Blank()

        // Star map selector
        if (this.allStarMaps.length > 1) {
          Select(this.allStarMaps.map((sm: StarMapMeta): SelectOption => ({ value: sm.title, icon: '' })))
            .selected(this.allStarMaps.findIndex(sm => sm.id === this.starmapId))
            .value(this.starmapTitle)
            .width(150)
            .onSelect((index: number) => {
              const sm = this.allStarMaps[index]
              if (sm) {
                this.starmapId = sm.id
                this.starmapTitle = sm.title
                this.loadStarMap()
              }
            })
            .margin({ right: 12 })
        }

        // Zoom controls
        Row() {
          Button('-')
            .fontSize(18)
            .width(36)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surface)
            .fontColor(this.themeConfig.colors.textPrimary)
            .onClick(() => {
              this.zoomScale = Math.max(0.3, this.zoomScale - 0.1)
            })

          Text(`${Math.round(this.zoomScale * 100)}%`)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .width(50)
            .textAlign(TextAlign.Center)

          Button('+')
            .fontSize(18)
            .width(36)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surface)
            .fontColor(this.themeConfig.colors.textPrimary)
            .onClick(() => {
              this.zoomScale = Math.min(3, this.zoomScale + 0.1)
            })

          Button('重置')
            .fontSize(12)
            .height(36)
            .backgroundColor(this.themeConfig.colors.surface)
            .fontColor(this.themeConfig.colors.textPrimary)
            .margin({ left: 8 })
            .onClick(() => {
              this.zoomScale = 1
              this.offsetX = 0
              this.offsetY = 0
            })
        }
      }
      .width('100%')
      .constraintSize({ maxWidth: this.adaptiveState.contentMaxWidth })
      .padding({ left: this.adaptiveState.pagePadding, right: this.adaptiveState.pagePadding, top: 16, bottom: 12 })

      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
            .color(this.themeConfig.colors.primary)
          Text('加载中...')
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ top: 12 })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .fontColor(this.themeConfig.colors.error)
            .margin({ bottom: 16 })

          Button('重试')
            .onClick(() => this.loadStarMap())
            .backgroundColor(this.themeConfig.colors.primary)
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      } else if (this.graph) {
        Stack() {
          // Canvas for edges
          Canvas(this.buildCanvasContext())
            .width('100%')
            .height('100%')
            .backgroundColor(this.themeConfig.colors.background)
            .onReady(() => {
              this.drawEdges()
            })

          // Nodes layer
          ForEach(this.graph.nodes, (node: StarMapNode) => {
            // 从 layoutNodes 查找坐标
            const layoutNode = this.layoutNodes.find(ln => ln.nodeId === node.id)
            const nodeX = layoutNode?.x ?? 0
            const nodeY = layoutNode?.y ?? 0
            const nodeWidth = layoutNode?.width ?? 160
            const nodeHeight = layoutNode?.height ?? 80

            Column() {
              Circle({ width: 56, height: 56 })
                .fill(this.getNodeColor(node))
                .stroke(this.selectedNodeId === node.id ? '#FF6B6B' : 'transparent')
                .strokeWidth(4)
                .shadow({
                  radius: this.draggingNodeId === node.id ? 12 : (this.selectedNodeId === node.id ? 8 : 4),
                  color: this.draggingNodeId === node.id ? '#40000000' : (this.selectedNodeId === node.id ? '#40FF6B6B' : '#20000000'),
                  offsetX: 0,
                  offsetY: this.draggingNodeId === node.id ? 4 : 2
                })

              Text(node.title)
                .fontSize(13)
                .fontWeight(this.selectedNodeId === node.id ? FontWeight.Bold : FontWeight.Normal)
                .fontColor(this.themeConfig.colors.textPrimary)
                .margin({ top: 6 })
                .maxLines(2)
                .textAlign(TextAlign.Center)
                .width(80)
            }
            .position({
              x: (nodeX * this.zoomScale) + this.offsetX - nodeWidth / 2 + this.wobbleX(node.id, this.motionTick),
              y: (nodeY * this.zoomScale) + this.offsetY - nodeHeight / 2 + this.wobbleY(node.id, this.motionTick)
            })
            .scale(
              this.draggingNodeId === node.id
                ? { x: this.dragLiftScale, y: this.dragLiftScale }
                : { x: 1, y: 1 }
            )
            .onClick(() => {
              this.selectNode(node.id)
            })
            .gesture(
              PanGesture()
                .onActionStart(() => {
                  this.draggingNodeId = node.id
                })
                .onActionUpdate((event: GestureEvent) => {
                  // 更新 layoutNode 坐标，不修改 StarMapNode
                  const layoutIndex = this.layoutNodes.findIndex(ln => ln.nodeId === node.id)
                  if (layoutIndex >= 0) {
                    this.layoutNodes[layoutIndex].x += event.offsetX / this.zoomScale
                    this.layoutNodes[layoutIndex].y += event.offsetY / this.zoomScale
                  }
                })
                .onActionEnd(() => {
                  this.draggingNodeId = ''
                  // debounce 300ms 后保存布局
                  setTimeout(() => {
                    if (this.starmapId && this.layoutNodes.length > 0) {
                      const layout: StarMapLayout = {
                        kind: 'Freeform',
                        nodes: this.layoutNodes
                      }
                      this.bridge.saveStarMapLayout(this.starmapId, layout)
                    }
                  }, 300)
                })
            )
          }, (node: StarMapNode) => node.id)

          // Node info panel — 宽屏下偏右，窄屏下左上角
          if (this.selectedNode) {
            this.NodeInfoPanel()
          }

          // Legend — 宽屏下右下角，窄屏下左下角
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
        .gesture(
          PanGesture()
            .onActionUpdate((event: GestureEvent) => {
              this.offsetX += event.offsetX
              this.offsetY += event.offsetY
            })
        )
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.themeConfig.colors.background)
  }

  @Builder
  NodeInfoPanel() {
    Column() {
      Row() {
        Text('节点信息')
          .fontSize(16)
          .fontWeight(FontWeight.Medium)
          .fontColor(this.themeConfig.colors.textPrimary)

        Blank()

        Text('✕')
          .fontSize(18)
          .fontColor(this.themeConfig.colors.textHint)
          .onClick(() => {
            this.deselectNode()
          })
      }
      .width('100%')
      .margin({ bottom: 12 })

      Column() {
        // Node type badge
        Row() {
          Circle({ width: 12, height: 12 })
            .fill(this.getNodeColor(this.selectedNode!))
            .margin({ right: 8 })

          Text(this.getNodeTypeName(this.selectedNode!.kind))
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textSecondary)
        }
        .margin({ bottom: 12 })

        // Node title
        Text(this.selectedNode!.title)
          .fontSize(22)
          .fontWeight(FontWeight.Bold)
          .fontColor(this.themeConfig.colors.textPrimary)
          .margin({ bottom: 8 })

        // Node description
        if (this.selectedNode!.description) {
          Text(this.selectedNode!.description!)
            .fontSize(14)
            .fontColor(this.themeConfig.colors.textSecondary)
            .margin({ bottom: 12 })
        }

        // Node coordinates — 从 layoutNodes 查找
        Row() {
          Text(`X: ${Math.round(this.layoutNodes.find(ln => ln.nodeId === this.selectedNodeId)?.x ?? 0)}`)
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
            .margin({ right: 16 })

          Text(`Y: ${Math.round(this.layoutNodes.find(ln => ln.nodeId === this.selectedNodeId)?.y ?? 0)}`)
            .fontSize(12)
            .fontColor(this.themeConfig.colors.textHint)
        }
        .margin({ bottom: 16 })

        // Related edges
        if (this.graph) {
          Text('关联关系')
            .fontSize(14)
            .fontWeight(FontWeight.Medium)
            .fontColor(this.themeConfig.colors.textPrimary)
            .margin({ bottom: 8 })

          ForEach(this.graph.edges.filter(e =>
            e.from === this.selectedNodeId || e.to === this.selectedNodeId
          ), (edge: StarMapEdge) => {
            Row() {
              Circle({ width: 8, height: 8 })
                .fill(this.themeConfig.colors.primary)
                .margin({ right: 8 })

              Column() {
                Text(edge.label || '未命名关系')
                  .fontSize(13)
                  .fontColor(this.themeConfig.colors.textPrimary)

                Text(this.getEdgeTypeName(edge.kind))
                  .fontSize(11)
                  .fontColor(this.themeConfig.colors.textHint)
              }
              .layoutWeight(1)

              Text(edge.from === this.selectedNodeId ? '→' : '←')
                .fontSize(14)
                .fontColor(this.themeConfig.colors.textSecondary)
            }
            .margin({ bottom: 8 })
          }, (edge: StarMapEdge) => edge.id)
        }
      }
    }
    .width(300)
    .padding(16)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(12)
    .shadow({ radius: 12, color: '#33000000', offsetX: 0, offsetY: 4 })
    .position({
      x: this.adaptiveState.sizeClass === WidthClass.Expanded ? this.stackWidth - 300 - 16 : 16,
      y: 16
    })
  }

  @Builder
  Legend() {
    Column() {
      Text('图例')
        .fontSize(12)
        .fontColor(this.themeConfig.colors.textHint)
        .margin({ bottom: 8 })

      Row() {
        this.LegendItem('#4A90D9', '角色')
        this.LegendItem('#F39C12', '地点')
        this.LegendItem('#9B59B6', '事件')
        this.LegendItem('#2ECC71', '主题')
      }
      .margin({ bottom: 4 })

      Row() {
        this.LegendItem('#E74C3C', '情节')
        this.LegendItem('#1ABC9C', '物品')
        this.LegendItem('#3498DB', '组织')
        this.LegendItem('#95A5A6', '自定义')
      }
    }
    .padding(12)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(8)
    .shadow({ radius: 4, color: '#1A000000', offsetX: 0, offsetY: 2 })
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
        .fontColor(this.themeConfig.colors.textSecondary)
    }
    .margin({ right: 12 })
  }

  @Builder
  GraphInfo() {
    Column() {
      if (this.graph) {
        Text(`${this.graph.nodes.length} 节点 · ${this.graph.edges.length} 连线`)
          .fontSize(12)
          .fontColor(this.themeConfig.colors.textHint)
      }
    }
    .padding(8)
    .backgroundColor(this.themeConfig.colors.surface)
    .borderRadius(4)
    .shadow({ radius: 2, color: '#1A000000', offsetX: 0, offsetY: 1 })
    .position({ x: this.stackWidth - 160 - 16, y: this.stackHeight - 40 - 16 })
  }

  private canvasCtx: CanvasRenderingContext2D | null = null

  buildCanvasContext(): CanvasRenderingContext2D {
    this.canvasCtx = new CanvasRenderingContext2D(new RenderingContextSettings(true))
    return this.canvasCtx
  }

  drawEdges() {
    if (!this.canvasCtx || !this.graph) return
    const ctx = this.canvasCtx

    ctx.clearRect(0, 0, ctx.width, ctx.height)

    // 构建 nodeId → wobble offset 映射
    const wobbleMap = new Map<string, WobbleOffset>()
    for (const node of this.graph.nodes) {
      const offset: WobbleOffset = {
        dx: this.wobbleX(node.id, this.motionTick),
        dy: this.wobbleY(node.id, this.motionTick)
      }
      wobbleMap.set(node.id, offset)
    }

    // 遍历 edgeRenders（Core 计算的连线几何）
    for (const edge of this.edgeRenders) {
      // 找到对应的 graph edge 以获取 from/to nodeId
      const graphEdge = this.graph.edges.find(e => e.id === edge.edgeId)
      if (!graphEdge) continue

      const fromId = graphEdge.from || ''
      const toId = graphEdge.to || ''
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
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.divider
      ctx.lineWidth = isSelected ? 2.5 : 1.5

      // 虚线样式
      if (graphEdge.kind === 'DependsOn' || graphEdge.kind === 'ConflictsWith') {
        ctx.setLineDash([6, 4])
      } else {
        ctx.setLineDash([])
      }
      ctx.stroke()
      ctx.setLineDash([])

      // 箭头（使用 edgeRenders 的箭头坐标 + wobble offset）
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
      ctx.strokeStyle = isSelected ? this.themeConfig.colors.primary : this.themeConfig.colors.divider
      ctx.lineWidth = isSelected ? 2.5 : 1.5
      ctx.stroke()

      // 边标签
      if (graphEdge.label && edge.labelX !== undefined && edge.labelY !== undefined) {
        const lx = (edge.labelX + (fromWobble.dx + toWobble.dx) / 2) * this.zoomScale + this.offsetX
        const ly = (edge.labelY + (fromWobble.dy + toWobble.dy) / 2) * this.zoomScale + this.offsetY
        ctx.font = '11px HarmonyOS Sans'
        ctx.fillStyle = isSelected ? this.themeConfig.colors.textPrimary : this.themeConfig.colors.textHint
        ctx.textAlign = 'center'
        ctx.textBaseline = 'bottom'
        ctx.fillText(graphEdge.label, lx, ly - 6)
      }
    }
  }
}
