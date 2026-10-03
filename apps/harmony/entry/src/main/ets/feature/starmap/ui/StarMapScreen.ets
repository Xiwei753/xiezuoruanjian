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
import {
  RecursiveSceneSource, EmbedFocusProbe,
  buildRecursiveSceneContext, collectEmbedFocusProbes
} from '../platform/StarMapGeometry'
import { DEFAULT_EMBED_DIAMETER } from '../platform/StarMapLayout'
import {
  VisualLodContext, FocusCandidate, ProjectedEmbedMetrics,
  projectEmbedMetrics, resolveFocusScenePath
} from '../platform/StarMapVisualLod'
import {
  StarMapSceneRegistry,
  CAMERA_SCALE_MIN,
  CAMERA_SCALE_MAX,
  computeCameraZoomAround
} from '../platform/StarMapViewport'
import { StarMapSelectionState } from '../platform/StarMapSelectionState'
import { StarMapScene } from './StarMapScene'

// 缩放边界：比例上下限来自 StarMapViewport 的相机常量，这里只留工具栏的步长
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

  // 全局视角相机（#818）：整棵递归星图只有这一台相机。
  // 工具栏 +/- 和画布双指捏合改的都是它；节点、子星图、嵌套子星图都只是相机里的内容，
  // 不存在"单独缩放某个子星图"这条路径。
  @State cameraScale: number = 1
  @State cameraOffsetX: number = 0
  @State cameraOffsetY: number = 0

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
  // 同时也是**根 StarMap 实际视口**（#820）：不是物理屏幕，是这张星图真正能画东西的那块区域。
  @State canvasWidth: number = 0
  @State canvasHeight: number = 0

  /**
   * 当前视觉焦点的 scenePath 描述（根为 'root'）（#820）。
   *
   * 焦点只决定"哪几颗 Embed 保持展开"，它**不是**页面、不是另一台相机、也不落盘：
   * 整棵树仍然只有一台全局相机，坐标系仍然是 root world → 全局相机 → 屏幕。
   * 窗口尺寸变化（分屏 / 折叠屏展开 / 横竖屏）时它保持不动，只更新视口宽高重算 LOD，
   * 不重新 fit 根图，也不把用户拽回根层。
   *
   * 只在这几个时机重算：全局相机变化、视口尺寸变化、捏合结束。
   * 绝不在 build() 或任何组件重建过程中写它（重建里改状态会触发重建死循环）。
   */
  @State focusScenePath: string = 'root'

  // 共享对象
  private bridge: IWriterCoreBridge = getBridge()
  private themeAdapter: HarmonyThemeAdapter = HarmonyThemeAdapter.getInstance()
  private themeObserver: ThemeObserver | null = null
  private adaptiveObserver: AdaptiveObserver | null = null
  private gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  // 全树唯一的选中状态（#816）：根和所有子 Scene 共用，选中身份是 scenePath + kind + itemId
  @State selectionState: StarMapSelectionState = new StarMapSelectionState()
  // 全树唯一的 Scene 注册表：双指入口按 scenePath 找归属层已作废（#818），
  // 这里只服务跨层连线（求最近公共祖先）和递归命中
  private sceneRegistry: StarMapSceneRegistry = new StarMapSceneRegistry()

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
   * 改全局相机。Scene 层（根或子）都通过根 Scene 的回调调到这里。
   */
  applyCamera(scale: number, offsetX: number, offsetY: number) {
    this.cameraScale = scale
    this.cameraOffsetX = offsetX
    this.cameraOffsetY = offsetY
    // 相机变了，每颗 Embed 的屏幕投影都变了，视觉焦点要重新判一次（#820）。
    // 结果不变时不写 @State，避免每帧捏合都触发一次无谓重建。
    this.recomputeVisualFocus()
  }

  /**
   * 重算视觉焦点（#820）。
   *
   * 纯显示行为：读注册表里的 Scene 快照 → 收集候选 Embed 的圆心位置和所属层比例 →
   * 交给 StarMapVisualLod 的纯函数判（覆盖率过 0.70 进、过 0.55 出，中间那段滞回不动）。
   *
   * 全程不写 Core、不新建页面、不动相机、不落盘；只在焦点真的变了时写一次 @State。
   * 注册表还没建起来（图没加载）时直接返回，保持 'root'。
   */
  private recomputeVisualFocus(): void {
    const rootScene: RecursiveSceneSource | null = this.sceneRegistry.getRoot()
    if (rootScene === null || this.canvasWidth <= 0 || this.canvasHeight <= 0) {
      return
    }
    const probes: EmbedFocusProbe[] = collectEmbedFocusProbes(
      buildRecursiveSceneContext(rootScene), this.canvasWidth, this.canvasHeight
    )
    const context: VisualLodContext = {
      viewportWidthVp: this.canvasWidth,
      viewportHeightVp: this.canvasHeight,
      focusScenePath: this.focusScenePath
    }
    const candidates: FocusCandidate[] = []
    for (const probe of probes) {
      const metrics: ProjectedEmbedMetrics = projectEmbedMetrics(
        DEFAULT_EMBED_DIAMETER, probe.ownerScale, context, 0
      )
      candidates.push({
        scenePath: probe.childScenePath,
        coverage: metrics.coverage,
        centerRatioX: probe.centerRatioX,
        centerRatioY: probe.centerRatioY
      })
    }
    const nextFocus: string = resolveFocusScenePath(this.focusScenePath, candidates)
    if (nextFocus !== this.focusScenePath) {
      this.focusScenePath = nextFocus
    }
  }

  /**
   * 任意一层的单指画布拖动都汇到这里，改的是唯一的全局相机（#818 复审）。
   *
   * 子星图不是独立窗口：手指落在根空白、子星图内部空白、二层子星图内部空白，
   * 拖的都是整张星图的视角，所以这里不关心是哪一层发起的。
   *
   * 注意是**目标值**不是增量：ArkUI PanGesture 的 event.offsetX/Y 是相对手势起点的
   * 累计位移，Scene 层已经算好了"起手基准 + 累计位移"，这里整份覆盖相机 offset。
   * 如果这里再写成 `cameraOffsetX + offsetX`，同一次手势的每一帧都会被叠加一遍。
   */
  panCameraTo(offsetX: number, offsetY: number) {
    this.applyCamera(this.cameraScale, offsetX, offsetY)
  }

  /**
   * 工具栏缩放：围绕可视画布中心改全局相机（#818）。
   *
   * 工具栏作用的是整棵树，不是"当前看进去的那张子星图"。
   */
  private zoomAround(centerX: number, centerY: number, nextScale: number) {
    const next = computeCameraZoomAround({
      scale: this.cameraScale,
      offsetX: this.cameraOffsetX,
      offsetY: this.cameraOffsetY
    }, centerX, centerY, nextScale)
    this.applyCamera(next.scale, next.offsetX, next.offsetY)
  }

  /** 放大一级（工具栏 +）：以可视画布中心为锚点 */
  zoomIn() {
    this.zoomAround(this.canvasWidth / 2, this.canvasHeight / 2,
      Math.min(CAMERA_SCALE_MAX, this.cameraScale + ZOOM_STEP))
  }

  /** 缩小一级（工具栏 −） */
  zoomOut() {
    this.zoomAround(this.canvasWidth / 2, this.canvasHeight / 2,
      Math.max(CAMERA_SCALE_MIN, this.cameraScale - ZOOM_STEP))
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
        // 全局相机：根 Scene 是相机本体（#818）
        cameraScale: this.cameraScale,
        cameraOffsetX: this.cameraOffsetX,
        cameraOffsetY: this.cameraOffsetY,
        onCameraChanged: (scale: number, offsetX: number, offsetY: number) => {
          this.applyCamera(scale, offsetX, offsetY)
        },
        // 任意一层空白处的单指拖动都汇回根相机（#818 复审：子星图不是独立窗口）
        onCameraPanTo: (offsetX: number, offsetY: number) => {
          this.panCameraTo(offsetX, offsetY)
        },
        sceneDepth: 0,
        // 根视口 + 视觉焦点：从这里原样透传给每一个递归 Scene（#820）。
        // 子 Scene 不许拿自己那个圆的尺寸当视口，也不许自己判断焦点。
        rootViewportWidth: this.canvasWidth,
        rootViewportHeight: this.canvasHeight,
        focusScenePath: this.focusScenePath,
        // 捏合结束时再判一次焦点：捏合过程中每帧都会改相机，
        // 但那一帧的树结构可能还没定形（子 Scene 可能正在重建），手势收尾才是干净的时机。
        onFocusRecomputeRequested: () => {
          this.recomputeVisualFocus()
        },
        bridge: this.bridge,
        themeConfig: this.themeConfig,
        motionConfig: this.motionConfig,
        motionProgress: this.motionProgress,
        gestureTracker: this.gestureTracker,
        // 全树共用的选中状态和 Scene 注册表：从根一路传到每个子 Scene（#816）
        selectionState: this.selectionState,
        sceneRegistry: this.sceneRegistry,
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
      // 视口变了（分屏 / 折叠屏展开 / 横竖屏）：只重算视口与视觉焦点（#820）。
      // 相机保持不动、focusScenePath 保持不动、不重新 fit 根图 —— 用户不会因为
      // 转个屏就被拽回根层看全局，那等于把正在写的内容从眼前搬走。
      this.recomputeVisualFocus()
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
