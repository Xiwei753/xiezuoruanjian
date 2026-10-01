import { Routes, StarMapRouteParams } from '../../../app/navigation/Routes'
import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { StarMapGraph, StarMapPathSegment, StarMapPoint } from '../../../corebridge/dto/StarMapDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ScreenRole, ActionSlot } from '../../../corebridge/dto/PlatformDtos'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { ScreenPolicyBridge } from '../../../ui/adaptive/ScreenPolicyBridge'
import { PlatformMotionConfig, defaultMotionConfig, shouldAnimate } from '../platform/StarMapMotion'
import Animator, { AnimatorResult } from '@ohos.animator'
import { StarMapGestureStateTracker } from '../platform/StarMapGestureState'
import { StarMapScene } from './StarMapScene'

// 缩放边界
const MIN_ZOOM: number = 0.3
const MAX_ZOOM: number = 3
const ZOOM_STEP: number = 0.1

@Component
export struct StarMapScreen {
  // 导航
  @Consume("navPathStack") navPathStack: NavPathStack

  // 根星图状态
  @State rootStarmapId: string = ''
  @State starmapId: string = ''
  @State starmapTitle: string = ''
  @State rootStarmapTitle: string = ''
  @State graph: StarMapGraph | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''

  // 根 Scene viewport（与 StarMapScene 双向绑定）
  @State zoomScale: number = 1
  @State offsetX: number = 0
  @State offsetY: number = 0
  @State selectedNodeId: string = ''

  // 主题和适配
  @State themeConfig: ThemeConfig = HarmonyThemeAdapter.getInstance().getCurrentTheme()
  @State adaptiveState: AdaptiveState = getAdaptiveState()
  @State useHdsNav: boolean = false
  @State headerActions: ActionSlot[] = []

  // 运动配置和动画
  @State motionConfig: PlatformMotionConfig = defaultMotionConfig()
  @State @Watch('onMotionProgressChange') motionProgress: number = 0
  private wobbleAnimator: AnimatorResult | null = null

  // Canvas 生命周期
  @State canvasActive: boolean = false

  // 加载序号：防止切层后旧异步结果覆盖新层
  private graphLoadGeneration: number = 0

  // 画布尺寸（用于 zoomAround 计算可视中心）
  @State canvasWidth: number = 0
  @State canvasHeight: number = 0

  // 共享对象
  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()

  async aboutToAppear() {
    hilog.info(0x0000, 'StarMapScreen', 'route name=StarMap')
    const paramsArr = this.navPathStack.getParamByName(Routes.StarMap) as StarMapRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.starmapId = params.starmapId
      this.rootStarmapId = params.starmapId
      this.starmapTitle = params.starmapTitle
      this.rootStarmapTitle = params.starmapTitle
      this.loadStarMap()
    } else {
      // 没有路由参数时不自动选择第一张星图，直接显示空状态
      this.isLoading = false
    }

    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    if (!this.useHdsNav) {
      const policy = await ScreenPolicyBridge.getInstance().resolveScreenPolicy(ScreenRole.StarMapDetail)
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

    // Canvas 激活：在启动 wobble 之前设置
    this.canvasActive = true

    // 启动 wobble 动画：用 Animator onFrame 逐帧回调驱动 motionProgress
    this.startWobbleAnimation()
  }

  aboutToDisappear() {
    this.canvasActive = false
    this.stopWobbleAnimation()
    this.gestureTracker.reset()
    if (this.themeObserver) {
      this.themeAdapter.removeObserver(this.themeObserver)
    }
    if (this.adaptiveObserver) {
      AdaptiveContext.getInstance().removeObserver(this.adaptiveObserver)
    }
  }

  /**
   * 用 ArkUI Animator 的 onFrame 逐帧回调驱动 motionProgress ∈ [0, 1]。
   * Animator 的 begin=0, end=1, iterations=-1（无限循环），easing='linear'。
   * onFrame 每帧把 progress 写入 @State motionProgress，触发 StarMapScene 重画 Canvas 边。
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
    const generation = ++this.graphLoadGeneration
    const requestedStarmapId = this.starmapId

    this.isLoading = true
    this.errorMessage = ''

    try {
      const result = await this.bridge.getStarMapGraph(requestedStarmapId)

      if (generation !== this.graphLoadGeneration ||
          requestedStarmapId !== this.starmapId) {
        return
      }

      if (result.success && result.data) {
        this.graph = result.data
      } else {
        this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_starmap').id)
      }
    } catch (err) {
      if (generation !== this.graphLoadGeneration ||
          requestedStarmapId !== this.starmapId) {
        return
      }
      this.errorMessage = getContext(this).resourceManager.getStringSync($r('app.string.error_load_starmap_detail').id)
    } finally {
      if (generation === this.graphLoadGeneration &&
          requestedStarmapId === this.starmapId) {
        this.isLoading = false
      }
    }
  }

  /**
   * 统一缩放函数：围绕指定屏幕点缩放，保持该点对应的画布坐标不变。
   */
  private zoomAround(screenX: number, screenY: number, nextScale: number) {
    const anchorCanvasX = (screenX - this.offsetX) / this.zoomScale
    const anchorCanvasY = (screenY - this.offsetY) / this.zoomScale
    this.offsetX = screenX - anchorCanvasX * nextScale
    this.offsetY = screenY - anchorCanvasY * nextScale
    this.zoomScale = nextScale
  }

  /** 放大一级（工具栏 +）：以可视画布中心为锚点 */
  zoomIn() {
    const centerX = this.canvasWidth / 2
    const centerY = this.canvasHeight / 2
    this.zoomAround(centerX, centerY, Math.min(MAX_ZOOM, this.zoomScale + ZOOM_STEP))
  }

  /** 缩小一级（工具栏 −） */
  zoomOut() {
    const centerX = this.canvasWidth / 2
    const centerY = this.canvasHeight / 2
    this.zoomAround(centerX, centerY, Math.max(MIN_ZOOM, this.zoomScale - ZOOM_STEP))
  }

  onMotionProgressChange() {
    // motionProgress 变化时由 StarMapScene 的 @Watch 自动处理重画
  }

  @Builder
  StarMapContent() {
    Stack() {
      // 使用 StarMapScene 组件替换所有节点/Embed/Canvas/手势逻辑
      StarMapScene({
        rootStarmapId: this.rootStarmapId,
        scenePath: [],  // 根 scene 的 path 为空
        starmapId: this.starmapId,
        graph: $graph,
        zoomScale: $zoomScale,
        offsetX: $offsetX,
        offsetY: $offsetY,
        sceneDepth: 0,
        selectedNodeId: $selectedNodeId,
        bridge: this.bridge,
        themeConfig: this.themeConfig,
        motionConfig: this.motionConfig,
        motionProgress: this.motionProgress,
        gestureTracker: this.gestureTracker,
        onSelectItem: (_path: StarMapPathSegment[], itemId: string) => { this.selectedNodeId = itemId },
        onCreateNode: (_path: StarMapPathSegment[], _starmapId: string, _title: string, _pos: StarMapPoint) => {},
        onCreateEmbed: (_path: StarMapPathSegment[], _starmapId: string, _title: string, _pos: StarMapPoint) => {},
        onCreateEdge: (_path: StarMapPathSegment[], _starmapId: string, _from: string, _to: string) => {},
        onMoveNode: (_path: StarMapPathSegment[], _starmapId: string, _nodeId: string, _pos: StarMapPoint) => {},
        onMoveEmbed: (_path: StarMapPathSegment[], _starmapId: string, _embedId: string, _pos: StarMapPoint) => {},
      })

      // 缩放控件：独立锚定右下角
      this.ZoomControls()

      // Loading 盖层
      if (this.isLoading) {
        Column() {
          LoadingProgress()
            .width(48)
            .height(48)
          Text($r('app.string.loading'))
            .fontSize(14)
            .margin({ top: 12 })
        }
        .justifyContent(FlexAlign.Center)
        .backgroundColor($r('sys.color.background_secondary'))
        .width('100%')
        .height('100%')
      }

      // Error 盖层
      if (this.errorMessage) {
        Column() {
          Text(this.errorMessage)
            .fontSize(16)
            .margin({ bottom: 16 })
          Button($r('app.string.retry'))
            .onClick(() => this.loadStarMap())
        }
        .justifyContent(FlexAlign.Center)
        .backgroundColor($r('sys.color.background_secondary'))
        .width('100%')
        .height('100%')
      }
    }
    .alignContent(Alignment.BottomEnd)
    .layoutWeight(1)
    .width('100%')
    .backgroundColor($r('sys.color.background_secondary'))
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      this.canvasWidth = Number(newArea.width)
      this.canvasHeight = Number(newArea.height)
    })
  }

  @Builder
  ZoomControls() {
    Row({ space: 8 }) {
      Button({ type: ButtonType.Circle, stateEffect: true }) {
        SymbolGlyph($r('sys.symbol.minus'))
          .fontSize(20)
      }
      .width(40)
      .height(40)
      .onClick(() => this.zoomOut())

      Button({ type: ButtonType.Circle, stateEffect: true }) {
        SymbolGlyph($r('sys.symbol.plus'))
          .fontSize(20)
      }
      .width(40)
      .height(40)
      .onClick(() => this.zoomIn())
    }
    .margin({ right: 16, bottom: 32 })
  }

  build() {
    if (this.useHdsNav) {
      HdsAppDestinationApi23({
        title: this.rootStarmapTitle,
        screenRole: ScreenRole.StarMapDetail,
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
      .title(this.rootStarmapTitle)
      .menus(buildNavMenuItems(this.headerActions))
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.StarMapDetail, 1)
      })
    }
  }
}
