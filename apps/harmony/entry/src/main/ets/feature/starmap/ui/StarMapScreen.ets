import { Routes, StarMapRouteParams } from '../../../app/navigation/Routes'
import { WideLayoutMode, WideLayoutState } from '../../../app/navigation/wide/WideLayoutMode'
import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { StarMapGraph, StarMapPathSegment, StarMapPoint, StarMapTargetPath } from '../../../corebridge/dto/StarMapDtos'
import { HarmonyThemeAdapter, ThemeConfig, ThemeObserver, ThemeObserverImpl } from '../../../ui/theme/HarmonyThemeAdapter'
import { getBridge } from '../../../app/AppContext'
import { AdaptiveContext, AdaptiveState, AdaptiveObserver, SimpleAdaptiveObserver, getAdaptiveState } from '../../../ui/adaptive/AdaptiveContext'
import { ScreenRole, ActionSlot, resolveScreenPolicy } from '../../../app/navigation/ScreenSemantics'
import { NavigationChromeStore } from '../../../app/navigation/NavigationChromeStore'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { HdsAppDestinationApi23 } from '../../../app/navigation/impl/api23/HdsAppDestinationApi23'
import { buildNavMenuItems } from '../../../app/navigation/NavigationHeaderActionMenu'
import { PlatformMotionConfig, defaultMotionConfig, shouldAnimate } from '../platform/StarMapMotion'
import Animator, { AnimatorResult } from '@ohos.animator'
import { StarMapGestureStateTracker } from '../platform/StarMapGestureState'
import {
  RecursiveSceneSource, EmbedFocusProbe,
  PlatformEdgeRender, PlatformNodeRect, resolveEdgeEndpointAnchor,
  buildRecursiveSceneContext, collectEmbedFocusProbes
} from '../platform/StarMapGeometry'
import {
  VisualLodContext, FocusCandidate, ProjectedEmbedMetrics,
  projectEmbedMetrics, resolveFocusScenePath
} from '../platform/StarMapVisualLod'
import {
  StarMapSceneRegistry,
  computeCameraZoomAround
} from '../platform/StarMapViewport'
import { StarMapSelectionState } from '../platform/StarMapSelectionState'
import { StarMapSelectionKind } from '../platform/StarMapSelectionState'
import { StarMapSyncBarrier } from '../platform/StarMapSyncBarrier'
import { AppContinuationService } from '../../../platform/continuity/AppContinuationService'
import { ContinuationPayload } from '../../../platform/continuity/ContinuationPayload'
import { StarMapScene } from './StarMapScene'
import { CompactPageState, CompactPageStateKind } from '../../../ui/components/CompactPageState'
import { AnnotationObjectRefDto } from '../../../corebridge/dto/AnnotationDtos'
import { AnnotationPanel } from '../../editor/annotation/AnnotationPanel'
import { WideStarMapToolRail } from './wide/WideStarMapHome'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

/**
 * 工具栏每次点击的**倍数**（#821）。
 *
 * #820 用 0.1 的加法步长：在 scale=2 时点一下是 2.1，在 scale=0.5 时
 * 是 0.6 —— 手感在两端完全不同，而且很快撞上原来的相机上限。
 * 改成乘法之后，每一档的相对变化一致，跨十几倍缩放都有同样的"点一下"体感；
 * 比例连续变化，只有 Number 真正上溢/下溢时 computeCameraZoomAround 才做数值规范化。
 */
const ZOOM_FACTOR: number = 1.2

interface StarMapSearchFocusGeometry {
  kind: 'node' | 'embed' | 'edge'
  itemId: string
  centerX: number
  centerY: number
}

/** 每个完整 route 各自保存的相机状态。 */
interface RouteCameraState {
  scale: number
  offsetX: number
  offsetY: number
}

@Component
export struct StarMapScreen {
  // 导航
  @Consume("navPathStack") navPathStack: NavPathStack

  // Issue #972：详情页可能从星图 Tab 打开，也可能从写作工作台（作品 Tab）打开。
  // 角色上报必须使用真实 owner Tab，否则 NavigationChromeStore 会把详情写进错误的标签缓存。
  @Prop screenTabIndex: number = 1

  // 根星图状态
  @State rootStarmapId: string = ''
  @State starmapId: string = ''
  /**
   * 当前活动画布的完整入口 route。空数组 = 根画布。
   * 每次进入 Embed 只追加一段；退出只移除最后一段。A→B→A / A→A 都是合法 route。
   */
  @State activePath: StarMapPathSegment[] = []
  @State starmapTitle: string = ''
  @State rootStarmapTitle: string = ''
  @State graph: StarMapGraph | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State private annotationProjectId: string = ''
  @State private compactAnnotationPanelOpen: boolean = false
  @State private starMapToolPanelOpen: boolean = false
  @State private selectedStarMapTool: string = 'ai'

  // 全局视角相机（#818/#1009）：当前活动画布只有这一台相机。
  // 工具栏 +/- 和画布双指捏合改的都是它；节点和入口都只是这张画布里的内容。
  // 切换 route 时按完整路径分别保存/恢复视角，不存在“单独缩放某个入口”这条路径。
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

  // Issue #982：搜索结果精确落到节点/边/嵌入/链接时，图加载完成后解析几何、选中并居中。
  private searchFocusKind: string = ''
  private searchFocusId: string = ''
  private searchFocusHandled: boolean = false
  private searchFocusTimerId: number = -1

  // 加载序号：防止切层后旧异步结果覆盖新层
  private graphLoadGeneration: number = 0
  // route 解析序号：连续点入口/返回时，只允许最后一次导航落地
  private routeRequestGeneration: number = 0
  /** 每个完整 route 自己的相机。退出时恢复进入前的视角，而不是复用子画布相机。 */
  private routeCameras: Map<string, RouteCameraState> = new Map<string, RouteCameraState>()
  // Issue #988：附近设备提交期间先等在途星图写入，提交后重载当前图。
  private unregisterPeerSync: (() => void) | null = null

  // 画布尺寸（用于 zoomAround 计算可视中心）
  // 同时也是**根 StarMap 实际视口**（#820）：不是物理屏幕，是这张星图真正能画东西的那块区域。
  @State canvasWidth: number = 0
  @State canvasHeight: number = 0

  /**
   * 当前视觉焦点的 scenePath 描述（根为 'root'）。
   *
   * 焦点只是"显示参考根"和接续状态的一部分，**不决定**任何渲染器、细节档位或
   * Embed 的展开/折叠；它**不是**页面、不是另一台相机、也不落盘。
   * 整棵树仍然只有一台全局相机，坐标系仍然是 root world → 全局相机 → 屏幕。
   * 窗口尺寸变化时它保持不动，不重新 fit 根图，也不把用户拽回根层。
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
    // Issue #988：附近设备同步不能只靠 Home 刷新；活跃星图要先等在途 CRUD 落盘，
    // 提交完成后按磁盘最新 graph.json 重载，避免旧内存图在后续保存时覆盖同步结果。
    this.unregisterPeerSync = StarMapSyncBarrier.getInstance().register({
      beforePeerCommit: async (paths: string[]): Promise<void> => {
        await this.closeStarMapStores(paths)
      },
      afterPeerCommit: async (paths: string[], applied: number,
        conflicts: number): Promise<void> => {
        await this.reloadAfterPeerCommit(paths, applied, conflicts)
      }
    })
    // Issue #988：用户点选节点/Embed/边时立即冻结当前星图快照，而不是只等相机变化。
    this.selectionState.setChangedListener((): void => {
      this.updateContinuationSnapshot()
    })
    const paramsArr = this.navPathStack.getParamByName(Routes.StarMap) as StarMapRouteParams[]
    const params = paramsArr && paramsArr.length > 0 ? paramsArr[paramsArr.length - 1] : null
    if (params) {
      this.starmapId = params.starmapId
      this.rootStarmapId = params.starmapId
      this.starmapTitle = params.starmapTitle
      this.rootStarmapTitle = params.starmapTitle
      if (params.focusKind !== undefined && params.focusId !== undefined) {
        this.searchFocusKind = params.focusKind
        this.searchFocusId = params.focusId
      }
      // Issue #988：接续恢复同一张星图的相机、视觉焦点和选中对象。
      if (params.cameraScale !== undefined && params.cameraScale > 0) {
        this.cameraScale = params.cameraScale
      }
      if (params.cameraOffsetX !== undefined) {
        this.cameraOffsetX = params.cameraOffsetX
      }
      if (params.cameraOffsetY !== undefined) {
        this.cameraOffsetY = params.cameraOffsetY
      }
      if (params.focusScenePath !== undefined && params.focusScenePath.length > 0) {
        this.focusScenePath = params.focusScenePath
      }
      if (params.selectedNodeId !== undefined && params.selectedNodeId.length > 0) {
        const selectedKind: StarMapSelectionKind =
          (params.selectedKind as StarMapSelectionKind | undefined) ?? 'node'
        this.selectionState.select([], selectedKind, params.selectedNodeId)
      }
      this.updateContinuationSnapshot()
      this.loadStarMap()
    } else {
      // 没有路由参数时不自动选择第一张星图，直接显示空状态
      this.isLoading = false
    }

    this.useHdsNav = PlatformApiResolver.getInstance().isApiAtLeast(23)
    if (!this.useHdsNav) {
      this.headerActions = resolveScreenPolicy(ScreenRole.StarMapDetail).actionSlots
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
    if (this.unregisterPeerSync !== null) {
      this.unregisterPeerSync()
      this.unregisterPeerSync = null
    }
    this.selectionState.setChangedListener(null)
    this.gestureTracker.reset()
    if (this.searchFocusTimerId !== -1) {
      clearTimeout(this.searchFocusTimerId)
      this.searchFocusTimerId = -1
    }
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

  private async reloadAfterPeerCommit(paths: string[], applied: number,
    conflicts: number): Promise<void> {
    if (applied <= 0) {
      return
    }
    const currentPrefix: string = `app/starmaps/${this.starmapId}/`
    const affectsCurrent: boolean = paths.some((path: string): boolean =>
      path === 'app/starmaps/index.json' || path === `${currentPrefix}graph.json` ||
      path.startsWith(currentPrefix))
    if (!affectsCurrent) {
      return
    }
    // 先让 Core 丢弃旧 Store（close 会 flush dirty），再从磁盘重载；
    // 只调用 getStarMapGraph 会命中原缓存，看不到 ETS 刚写入的对象。
    await this.closeStarMapStores(paths)
    await this.loadStarMap()
    if (conflicts > 0) {
      this.showSearchFocusToast(`附近设备同步完成，有 ${conflicts} 个星图冲突待处理`)
    }
  }

  private async closeStarMapStores(paths: string[]): Promise<void> {
    const ids: string[] = []
    const add = (id: string): void => {
      if (id.length > 0 && !ids.includes(id)) {
        ids.push(id)
      }
    }
    add(this.starmapId)
    paths.forEach((path: string): void => {
      const match = /^app\/starmaps\/([^/]+)\//.exec(path)
      if (match !== null) {
        add(match[1])
      }
    })
    for (const id of ids) {
      const result = await this.bridge.closeStarMapStore(id)
      if (!result.success) {
        const detail: string = result.userMessage ?? result.rawError ?? '星图缓存关闭失败'
        throw new Error(detail)
      }
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
        const metaResult = await this.bridge.getStarMap(requestedStarmapId)
        if (generation === this.graphLoadGeneration && metaResult.success && metaResult.data !== undefined) {
          this.annotationProjectId = metaResult.data.projectId ?? ''
        }
        this.updateContinuationSnapshot()
        // Issue #982：子 Scene 完成布局后会回调 onSceneGeometryReady；这里先排一次，
        // 视口尺寸或图数据哪个先到位都能触发同一条幂等定位。
        this.scheduleSearchFocus()
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

  private routeKey(path: StarMapPathSegment[]): string {
    if (path.length === 0) {
      return 'root'
    }
    const parts: string[] = []
    for (const segment of path) {
      parts.push(`${segment.type}:${segment.instanceId ?? ''}`)
    }
    return parts.join('/')
  }

  private saveRouteCamera(path: StarMapPathSegment[]): void {
    this.routeCameras.set(this.routeKey(path), {
      scale: this.cameraScale,
      offsetX: this.cameraOffsetX,
      offsetY: this.cameraOffsetY
    })
  }

  private restoreRouteCamera(path: StarMapPathSegment[]): void {
    const camera: RouteCameraState | undefined = this.routeCameras.get(this.routeKey(path))
    if (camera === undefined) {
      this.cameraScale = 1
      this.cameraOffsetX = 0
      this.cameraOffsetY = 0
      return
    }
    this.cameraScale = Number.isFinite(camera.scale) && camera.scale > 0 ? camera.scale : 1
    this.cameraOffsetX = Number.isFinite(camera.offsetX) ? camera.offsetX : 0
    this.cameraOffsetY = Number.isFinite(camera.offsetY) ? camera.offsetY : 0
  }

  /**
   * 按完整 route 切换当前活动画布。
   *
   * route 只追加一段 enterEmbed，解析和加载都交给 Core；同一 starmap 可以出现在
   * 任意多个路径位置，这里不做“已访问”拒绝。离开当前画布前先保存相机，
   * 返回时恢复原画布视角。
   */
  private async openActiveRoute(path: StarMapPathSegment[]): Promise<void> {
    const request: number = ++this.routeRequestGeneration
    const resolved = await this.bridge.resolveStarMapPath(this.rootStarmapId, path)
    if (request !== this.routeRequestGeneration) {
      return
    }
    if (!resolved.success || !resolved.data || !resolved.data.finalStarmapId) {
      this.showSearchFocusToast('无法进入该子星图')
      return
    }
    const targetStarmapId: string = resolved.data.finalStarmapId
    this.saveRouteCamera(this.activePath)
    // 先丢掉上一张图的几何，避免新 route 的 scenePath 与旧 graph 混一帧。
    this.graph = null
    this.activePath = path
    this.starmapId = targetStarmapId
    this.focusScenePath = 'root'
    this.selectionState.clear()
    this.restoreRouteCamera(path)
    await this.loadStarMap()
  }

  private enterEmbedRoute(path: StarMapPathSegment[]): void {
    void this.openActiveRoute(path)
  }

  /** 退出当前活动画布：只移除 route 最后一段，回到进入前的入口实例。 */
  private async exitActiveCanvas(): Promise<void> {
    if (this.activePath.length === 0) {
      this.navPathStack.pop()
      return
    }
    const request: number = ++this.routeRequestGeneration
    this.saveRouteCamera(this.activePath)
    const parentPath: StarMapPathSegment[] = this.activePath.slice(0, -1)
    let parentStarmapId: string = this.rootStarmapId
    if (parentPath.length > 0) {
      const resolved = await this.bridge.resolveStarMapPath(this.rootStarmapId, parentPath)
      if (request !== this.routeRequestGeneration) {
        return
      }
      if (!resolved.success || !resolved.data || !resolved.data.finalStarmapId) {
        this.showSearchFocusToast('无法返回上层星图')
        return
      }
      parentStarmapId = resolved.data.finalStarmapId
    }
    this.graph = null
    this.activePath = parentPath
    this.starmapId = parentStarmapId
    this.focusScenePath = 'root'
    this.selectionState.clear()
    this.restoreRouteCamera(parentPath)
    await this.loadStarMap()
  }

  private isWideLayout(): boolean {
    return WideLayoutState.getInstance().current() === WideLayoutMode.Wide
  }

  private selectStarMapTool(key: string): void {
    this.selectedStarMapTool = key
    this.starMapToolPanelOpen = true
  }

  private toggleStarMapToolPanel(): void {
    this.starMapToolPanelOpen = !this.starMapToolPanelOpen
  }

  private selectedStarMapAnnotationTarget(): AnnotationObjectRefDto {
    const selection = this.selectionState.getSelection()
    if (selection.itemId.length > 0 && (selection.kind === 'node' || selection.kind === 'embed')) {
      return {
        kind: selection.kind === 'node' ? 'starMapNode' : 'starMapEmbed',
        projectId: this.annotationProjectId,
        objectId: selection.itemId
      }
    }
    return {
      kind: 'project',
      projectId: this.annotationProjectId,
      objectId: this.annotationProjectId
    }
  }

  /**
   * Issue #982：搜索结果跳转星图对象。
   * 不把「打开星图」当成功：必须把稳定 Core id 解析成当前根 Scene 的画布几何，
   * 写入全树共享选中态，并按几何中心移动全局相机。link / hyperlink 没有独立位置，
   * 解析 source 到所属 node/embed 后选中/居中宿主对象。
   */
  private scheduleSearchFocus(): void {
    if (this.searchFocusHandled || this.searchFocusKind.length === 0 ||
      this.searchFocusId.length === 0 || this.searchFocusTimerId !== -1) {
      return
    }
    // 不在 build / 图数据 watch 的同步回调里直接改相机和选中，延后到下一帧。
    this.searchFocusTimerId = setTimeout((): void => {
      this.searchFocusTimerId = -1
      this.applySearchFocus()
    }, 0)
  }

  private applySearchFocus(): void {
    if (this.searchFocusHandled || this.searchFocusKind.length === 0 || this.searchFocusId.length === 0) {
      return
    }
    if (this.graph === null || this.canvasWidth <= 0 || this.canvasHeight <= 0) {
      return
    }
    const root: RecursiveSceneSource | null = this.sceneRegistry.getRoot()
    if (root === null) {
      return
    }
    // 根 Scene 句柄已注册但几何还没产出时继续等 onSceneGeometryReady / onAreaChange。
    if (root.rects.length === 0 && (this.graph.nodes.length > 0 || this.graph.embeds.length > 0)) {
      return
    }
    const focus: StarMapSearchFocusGeometry | null = this.resolveSearchFocusGeometry(root)
    if (focus === null) {
      // 图已经可用但目标不在当前星图（已删除/跨层不可见）：明确反馈，而不是只打开星图。
      this.searchFocusHandled = true
      this.showSearchFocusToast('未能在星图中定位目标')
      return
    }

    this.selectionState.select(this.activePath, focus.kind, focus.itemId)
    if (this.sceneRegistry !== null) {
      this.sceneRegistry.redrawAllEdges()
    }
    const cameraScale: number = this.cameraScale > 0 ? this.cameraScale : 1
    this.applyCamera(
      cameraScale,
      this.canvasWidth / 2 - focus.centerX * cameraScale,
      this.canvasHeight / 2 - focus.centerY * cameraScale
    )
    this.searchFocusHandled = true
  }

  private resolveSearchFocusGeometry(
    root: RecursiveSceneSource
  ): StarMapSearchFocusGeometry | null {
    if (this.searchFocusKind === 'node' || this.searchFocusKind === 'embed') {
      const rect: PlatformNodeRect | null = this.findFocusRect(root, this.searchFocusId)
      if (rect === null) {
        return null
      }
      return {
        kind: this.searchFocusKind === 'embed' ? 'embed' : 'node',
        itemId: this.searchFocusId,
        centerX: rect.x + rect.width / 2,
        centerY: rect.y + rect.height / 2
      }
    }

    if (this.searchFocusKind === 'edge') {
      const edge: PlatformEdgeRender | undefined =
        root.edges.find((item: PlatformEdgeRender): boolean => item.edgeId === this.searchFocusId)
      if (edge === undefined) {
        return null
      }
      return {
        kind: 'edge',
        itemId: edge.edgeId,
        // 边的中心必须是线段中点，不能只选中不移动相机。
        centerX: (edge.startX + edge.endX) / 2,
        centerY: (edge.startY + edge.endY) / 2
      }
    }

    if (this.searchFocusKind === 'link' || this.searchFocusKind === 'hyperlink') {
      const source: StarMapTargetPath | null = this.resolveLinkSource(this.searchFocusKind, this.searchFocusId)
      if (source === null) {
        return null
      }
      const ownerId: string = this.resolveLinkOwnerId(source, root)
      if (ownerId.length === 0) {
        return null
      }
      const rect: PlatformNodeRect | null = this.findFocusRect(root, ownerId)
      if (rect === null) {
        return null
      }
      return {
        // link/hyperlink 没有独立坐标，选中并居中它所属的 node / embed。
        kind: root.embedInstanceIds.has(ownerId) ? 'embed' : 'node',
        itemId: ownerId,
        centerX: rect.x + rect.width / 2,
        centerY: rect.y + rect.height / 2
      }
    }
    return null
  }

  private findFocusRect(root: RecursiveSceneSource, itemId: string): PlatformNodeRect | null {
    const rect: PlatformNodeRect | undefined =
      root.rects.find((item: PlatformNodeRect): boolean => item.nodeId === itemId)
    return rect !== undefined ? rect : null
  }

  private resolveLinkSource(kind: string, id: string): StarMapTargetPath | null {
    if (this.graph === null) {
      return null
    }
    if (kind === 'link') {
      const link = this.graph.links.find((item): boolean => item.linkId === id)
      return link !== undefined ? link.source : null
    }
    const hyperlink = this.graph.hyperlinks.find((item): boolean => item.hyperlinkId === id)
    return hyperlink !== undefined ? hyperlink.source : null
  }

  /**
   * 解析 link/hyperlink 的 source 到当前根画布上可绘制的对象。
   * 优先复用边端点同一套规则（本地 node/anchor，深路径取第一段 enterEmbed/enterPortal），
   * 规则不适用时再退回 source.target.nodeId。
   */
  private resolveLinkOwnerId(source: StarMapTargetPath, root: RecursiveSceneSource): string {
    const visibleAnchor: string | null = resolveEdgeEndpointAnchor(source, this.starmapId)
    if (visibleAnchor !== null && this.findFocusRect(root, visibleAnchor) !== null) {
      return visibleAnchor
    }
    const directNodeId: string | null = source.target.nodeId
    if (directNodeId !== null && this.findFocusRect(root, directNodeId) !== null) {
      return directNodeId
    }
    return ''
  }

  private showSearchFocusToast(message: string): void {
    try {
      this.getUIContext().getPromptAction().showToast({ message: message })
    } catch (_e) {
      // Toast 失败不影响星图本身打开。
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
    this.updateContinuationSnapshot()
  }

  // Issue #988：星图接续状态只带稳定 ID、相机和焦点，不复制画布数据。
  private updateContinuationSnapshot(): void {
    if (this.starmapId.length === 0) {
      return
    }
    const selection = this.selectionState.getSelection()
    const payload: ContinuationPayload = {
      screen: 'starmap',
      tabIndex: 1,
      starmapId: this.starmapId,
      starmapTitle: this.starmapTitle,
      focusScenePath: this.focusScenePath,
      cameraScale: this.cameraScale,
      cameraOffsetX: this.cameraOffsetX,
      cameraOffsetY: this.cameraOffsetY
    }
    if (this.rootStarmapId !== this.starmapId) {
      payload.parentStarmapId = this.rootStarmapId
      payload.parentStarmapTitle = this.rootStarmapTitle
    }
    if (selection.kind !== 'none') {
      payload.selectedKind = selection.kind
      payload.selectedNodeId = selection.itemId
    }
    AppContinuationService.getInstance().updateCurrentScreen(payload)
  }

  /**
   * 重算视觉焦点（#820/#821）。
   *
   * 纯显示行为：读注册表里的 Scene 快照 → 收集候选 Embed 的圆心位置和所属层比例 →
   * 交给 StarMapVisualLod 的纯函数判（覆盖率过 0.70 进、过 0.55 出，中间那段滞回不动）。
   *
   * 覆盖率只依赖 Embed 的投影直径和根视口，与形状无关：Embed 的 world 尺寸
   * 恒为 DEFAULT_EMBED_DIAMETER，焦点判定不切换任何显示形态。
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
      const metrics: ProjectedEmbedMetrics = projectEmbedMetrics(probe.ownerScale, context)
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

  /** 放大一级（工具栏 +）：以可视画布中心为锚点，乘法步进（#821） */
  zoomIn() {
    this.zoomAround(this.canvasWidth / 2, this.canvasHeight / 2,
      this.cameraScale * ZOOM_FACTOR)
  }

  /** 缩小一级（工具栏 −）：乘法步进，连续缩放，只有数值上下溢时才规范化 */
  zoomOut() {
    this.zoomAround(this.canvasWidth / 2, this.canvasHeight / 2,
      this.cameraScale / ZOOM_FACTOR)
  }

  onMotionProgressChange() {
    // motionProgress 变化时由 StarMapScene 的 @Watch 自动处理重画
  }

  @Builder
  StarMapContent() {
    if (this.isWideLayout()) {
      Row() {
        this.StarMapCanvas()

        if (this.starMapToolPanelOpen) {
          Column() {
            this.StarMapToolPanel()
          }
            .width(320)
            .height('100%')
        }

        WideStarMapToolRail({
          selectedToolKey: this.selectedStarMapTool,
          panelVisible: this.starMapToolPanelOpen,
          onSelectTool: (key: string): void => {
            this.selectStarMapTool(key)
          },
          onTogglePanel: (): void => {
            this.toggleStarMapToolPanel()
          }
        })
          .width(56)
          .height('100%')
      }
      .width('100%')
      .height('100%')
    } else {
      this.StarMapCanvas()
    }
  }

  @Builder
  StarMapCanvas() {
    Stack() {
      // 使用 StarMapScene 组件替换所有节点/Embed/Canvas/手势逻辑
      StarMapScene({
        rootStarmapId: this.rootStarmapId,
        // 当前活动画布的完整 route；根画布为空数组。
        scenePath: this.activePath,
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
        // 活动画布始终按正常矢量几何显示，不把子星图缩进上一层的入口圆壳。
        sceneDepth: 0,
        onEnterEmbed: (path: StarMapPathSegment[]): void => {
          this.enterEmbedRoute(path)
        },
        // 捏合结束时再判一次焦点：捏合过程中每帧都会改相机，
        // 但那一帧的投影可能还没稳定，手势收尾才是干净的时机。
        onFocusRecomputeRequested: () => {
          this.recomputeVisualFocus()
        },
        // Issue #982：根 Scene 完成几何重算后通知页面把搜索目标解析成相机/选中。
        onSceneGeometryReady: () => {
          this.scheduleSearchFocus()
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

      if (this.activePath.length > 0) {
        Button('返回上层')
          .fontSize(13)
          .height(36)
          .padding({ left: 14, right: 14 })
          .backgroundColor(this.themeConfig.colors.surfaceContainerHigh)
          .fontColor(this.themeConfig.colors.onSurface)
          .onClick(() => {
            void this.exitActiveCanvas()
          })
          .position({ x: 12, y: 12 })
      }

      if (!this.isWideLayout() && this.annotationProjectId.length > 0 &&
          this.selectionState.hasSelection() &&
          (this.selectionState.selectedKind === 'node' || this.selectionState.selectedKind === 'embed')) {
        Column({ space: 8 }) {
          Button(this.compactAnnotationPanelOpen ? '收起标注' : '标注选中项')
            .onClick(() => {
              this.compactAnnotationPanelOpen = !this.compactAnnotationPanelOpen
            })
          if (this.compactAnnotationPanelOpen) {
            AnnotationPanel({
              projectId: this.annotationProjectId,
              target: this.selectedStarMapAnnotationTarget()
            })
              .width(340)
          }
        }
        .align(Alignment.TopStart)
        .padding(12)
      }

      // 缩放控件：独立锚定右下角
      this.ZoomControls()

      // Loading 盖层（#932：复用窄屏统一状态组件）
      if (this.isLoading) {
        Column() {
          CompactPageState({
            kind: CompactPageStateKind.Loading,
            title: getContext(this).resourceManager.getStringSync($r('app.string.loading').id)
          })
        }
        .backgroundColor($r('sys.color.background_secondary'))
        .width('100%')
        .height('100%')
      }

      // Error 盖层（#932：复用窄屏统一状态组件）
      if (this.errorMessage) {
        Column() {
          CompactPageState({
            kind: CompactPageStateKind.Error,
            title: this.errorMessage,
            onRetry: () => {
              this.loadStarMap()
            }
          })
        }
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
      // Issue #982：视口尺寸是相机居中的必要条件；图可能先于尺寸加载完成。
      this.scheduleSearchFocus()
      // 视口变了（分屏 / 折叠屏展开 / 横竖屏）：**只**更新视口尺寸。
      // 相机不动、focusScenePath 不动、不重新 fit 根图 —— 用户不会因为
      // 转个屏就被拽回根层看全局，那等于把正在写的内容从眼前搬走。
      //
      // 这里也**不能**顺手重算视觉焦点（#820 复核）：视口尺寸变化不改变任何
      // 画布内容或渲染档位。焦点是"用户在盯哪个子星图"，折叠屏展开一下、
      // 分屏拖宽一点就换焦点是没有理由的。
      // 焦点只在相机平移 / 缩放 / 捏合结束时才重算。
    })
  }

  @Builder
  StarMapToolPanel() {
    Column() {
      Row() {
        Text(this.selectedStarMapTool === 'tags' ? '标签' : 'AI')
          .fontSize(15)
          .fontWeight(FontWeight.Medium)
          .fontColor($r('sys.color.font_primary'))
        Blank()
        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Text('›')
            .fontSize(16)
            .fontColor($r('sys.color.icon_secondary'))
        }
        .width(28)
        .height(28)
        .borderRadius(DesignTokens.radiusPill)
        .backgroundColor($r('sys.color.comp_background_gray'))
        .onClick(() => this.toggleStarMapToolPanel())
      }
      .width('100%')
      .height(52)
      .padding({ left: 14, right: 12 })
      .alignItems(VerticalAlign.Center)

      if (this.selectedStarMapTool === 'tags') {
        if (this.annotationProjectId.length > 0) {
          AnnotationPanel({
            projectId: this.annotationProjectId,
            target: this.selectedStarMapAnnotationTarget()
          })
            .width('100%')
            .layoutWeight(1)
        } else {
          Text('作品标签暂不可用')
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
            .padding(16)
        }
      } else {
        Column({ space: 8 }) {
          Text('AI 尚未启用')
            .fontSize(13)
            .fontWeight(FontWeight.Medium)
            .fontColor($r('sys.color.font_primary'))
          Text('当前版本没有可用的 AI 会话能力。')
            .fontSize(11)
            .fontColor($r('sys.color.font_tertiary'))
            .lineHeight(18)
        }
        .width('100%')
        .layoutWeight(1)
        .padding(16)
        .alignItems(HorizontalAlign.Start)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor($r('sys.color.background_secondary'))
    .border({ width: { left: 1 }, color: $r('sys.color.comp_divider') })
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
        screenTabIndex: this.screenTabIndex,
        isRoot: false,
        onBack: () => {
          if (this.activePath.length > 0) {
            void this.exitActiveCanvas()
          } else {
            this.navPathStack.pop()
          }
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
        if (this.activePath.length > 0) {
          void this.exitActiveCanvas()
        } else {
          this.navPathStack.pop()
        }
        return true
      })
      .title(this.rootStarmapTitle)
      .menus(buildNavMenuItems(this.headerActions))
      .onShown(() => {
        NavigationChromeStore.getInstance().reportScreenRole(ScreenRole.StarMapDetail, this.screenTabIndex)
      })
    }
  }
}
