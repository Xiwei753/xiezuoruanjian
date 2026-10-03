// ─── StarMapEmbedScene.ets ───
// Embed 内部内容区的递归桥梁组件。
// StarMapScene 渲染 Embed 时，Embed 的 innerContent 区域使用 StarMapEmbedScene，
// StarMapEmbedScene 内部递归创建 child StarMapScene。
//
// 职责：
// - 加载子星图 graph（通过 bridge.getStarMapGraph）
// - 把全局相机原样传给 child StarMapScene
// - 将子 StarMapScene 嵌入 Embed 的 innerContent 区域
//
// #818 之后这里不再持有任何缩放状态：
// 用户缩放只有一台全局相机（在 StarMapScreen 上），子 Scene 是相机里的内容，
// 没有"这个子图被用户拧了几倍"这回事。子 Scene 的 fitScale 和居中偏移由它自己
// 按父 Embed 的可用区域算一次，属于布局适配，用户改不了。
// 这里只把父层算好的 inheritedScale（累计显示比例）继续往下传，让整棵树的
// 有效比例是 camera × 各层 fit 的连乘，而不是每层各缩各的。
// 同样也不再持有子场景的选中状态：全树共用父层传下来的 selectionState。
//
// 本组件在相机状态链上的唯一职责是"不弄丢同步"：
// StarMapScreen @State → 根 StarMapScene @Prop → 本组件 @Prop → child StarMapScene @Prop。
// 中间任何一层写成普通字段，同步就在那一层断掉；子 Scene 再拿旧 offset 当 Pan
// 的起手基准，全局相机就会跳回旧位置。所以三个相机数值必须是 @Prop（#818 复审）。
//
// 输入侧本组件同样不持有任何识别状态（#818 复审）：
// ArkUI 的 onChildTouchTest 只看得到显式设了 .id() 的命名节点，稳定 id 由父层
// StarMapScene 用 StarMapGeometry.describeEmbedInputNodeId / describeSceneInputNodeId
// 生成（带 scenePath，同一个 instanceId 在每层父图里各出现一次也不会重名）。
// 这里既不新增 Pinch 也不新增 Pan：两指永远只有根 Scene 那一份 PinchGesture，
// 单指归属永远由 resolveRecursiveHit 决定。
//
// #820：本组件只在父层 LOD 为 expanded 时才存在——父层是 collapsed 时
// StarMapEmbedItem 的条件渲染根本不挂这个组件（不是挂上再隐藏），
// 所以深层递归的渲染开销是真的降下来了。子 Scene 的局部 fit 仍然只算一次。

import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import {
  StarMapGraph, StarMapEmbed, StarMapPoint, StarMapPathSegment
} from '../../../corebridge/dto/StarMapDtos'
import { ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import { PlatformMotionConfig } from '../platform/StarMapMotion'
import { StarMapGestureStateTracker } from '../platform/StarMapGestureState'
import { StarMapSceneRegistry } from '../platform/StarMapViewport'
import { StarMapSelectionState } from '../platform/StarMapSelectionState'
import { StarMapScene } from './StarMapScene'

@Component
export struct StarMapEmbedScene {
  // ─── @Prop：从父组件传入 ───
  /** 根星图 ID */
  @Prop rootStarmapId: string = ''
  /** 当前 scene 的路径段（不含当前 Embed 的 segment） */
  @Prop scenePath: StarMapPathSegment[] = []
  /** 当前 Embed 实例数据 */
  @Prop embed: StarMapEmbed = {
    instanceId: '',
    targetStarmapId: '',
    label: null,
    position: { x: 0, y: 0 },
    hostPath: {
      starmapId: '',
      segments: [],
      target: {
        type: 'starmap', nodeId: null, anchorId: null,
        projectId: null, volumeId: null, chapterId: null,
        rangeStart: null, rangeEnd: null,
        entityType: null, entityId: null, uri: null
      }
    },
    provenance: {
      source: 'Human', sourceId: null,
      generatedBy: null, promptId: null,
      reviewStatus: 'Accepted', createdFromAnchor: null
    },
    createdAt: 0, updatedAt: 0
  }
  /** 当前 scene 的深度 */
  @Prop sceneDepth: number = 0
  /** 主题配置 */
  @Prop themeConfig: ThemeConfig = {
    mode: 'light', colorSource: 'built_in',
    selectedPaletteId: '', selectedBuiltinThemeId: '',
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
    fontSize: 14, fontFamily: 'HarmonyOS Sans', lineHeight: 1.5
  }
  /** 运动配置 */
  @Prop motionConfig: PlatformMotionConfig = {
    enabled: true, reduceMotion: false,
    idleWobbleEnabled: true, idleAmplitudeVp: 2.0,
    idlePeriodMs: 4200, dragLiftScale: 1.04,
    settleDurationMs: 220
  }
  /** 动画进度 */
  @Prop motionProgress: number = 0

  /**
   * 全局相机比例：原样往下传，子 Scene 只是相机里的内容（#818）。
   *
   * 三个相机数值必须是 @Prop，不能退回普通字段。ArkUI V1 只有 @Prop 才是
   * 父 → 子的单向同步；普通字段只是拿父值做一次初始化，之后父层改了不会同步下来。
   * 少了这一层同步，子 Scene 读到的 this.cameraOffsetX/Y 会停在初始化时的旧值，
   * 而"拖画布"正是拿它当本次手势的起手基准：
   *
   *   根层先拖到 cameraOffsetX = 100 → 在子星图里起手 Pan（读到旧值 0）
   *   → event.offsetX = 5 → 目标 0 + 5 = 5 → 全局相机从 100 突然跳回 5
   *
   * 而且 Pan 只改 offset、不改 scale，指望 inheritedScale 顺带刷新是不成立的：
   * offset 和 scale 是两件事。这里不需要 @Watch，本组件自己不按相机值做副作用，
   * 只负责透传，让父值变化能同步到下面那份 child StarMapScene 就够了。
   */
  @Prop cameraScale: number = 1
  /** 全局相机 X 偏移：必须是 @Prop，否则子层 Pan 起手基准会读到旧值（#818 复审） */
  @Prop cameraOffsetX: number = 0
  /** 全局相机 Y 偏移：必须是 @Prop，否则子层 Pan 起手基准会读到旧值（#818 复审） */
  @Prop cameraOffsetY: number = 0
  /**
   * 根 StarMap 实际可绘制内容区宽高（vp），原样透传（#820）。
   *
   * 必须是 @Prop：LOD 判定读的就是这两个值，写成普通字段等于把子 Scene 的视口
   * 冻在创建那一刻，分屏 / 折叠屏展开之后子层永远按旧视口判展开。
   *
   * 子 Scene **不许**拿自己这个圆当根视口：那样"这层还剩多少展开空间"
   * 就是自己算自己，永远不够，递归会一路折到底。
   */
  @Prop rootViewportWidth: number = 0
  /** 根视口高度（vp），语义同 rootViewportWidth（#820）。 */
  @Prop rootViewportHeight: number = 0
  /**
   * 当前视觉焦点的 scenePath 描述（#820）：原样透传。
   * 焦点链上的 Embed 强制展开，子 Scene 靠它知道自己是不是"用户正在看的那一层"。
   */
  @Prop focusScenePath: string = 'root'

  // ─── 普通属性 ───
  bridge: IWriterCoreBridge | null = null
  gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  /**
   * 全树共用的选中状态（#816：不再每层各存一份 selectedNodeId）。
   * @State 持有 @Observed 对象：值只建一次，往下传的是同一个引用。
   */
  @State selectionState: StarMapSelectionState = new StarMapSelectionState()
  /** 全树共用的 Scene 注册表：跨层连线和递归命中靠它 */
  sceneRegistry: StarMapSceneRegistry | null = null
  /** 改全局相机的回调：原样往下传 */
  onCameraChanged: (scale: number, offsetX: number, offsetY: number) => void =
    (_scale: number, _offsetX: number, _offsetY: number) => {}
  /**
   * 祖先累计显示比例：父 StarMapScene 的 viewportScaleValue()（#818 复审）。
   *
   * child StarMapScene 的有效比例 = 它的局部 fitScale × 这一份。
   * 少了它，相机放大时只有 Embed 圆壳变大，圆里的节点不跟着放大。
   */
  @Prop inheritedScale: number = 1
  /**
   * 拖动画布的回调：原样往下传，最终只改根全局相机（#818 复审）。
   *
   * 子星图不是独立窗口，手指在这个子星图的空白里拖，动的也应该是整张星图的视角，
   * 所以这里不新增平移状态，只把目标 offset 一路回传给根。
   * 传的是目标值而不是增量：根必须整份覆盖，不能在现有相机上再相加。
   */
  onCameraPanTo: (offsetX: number, offsetY: number) => void =
    (_offsetX: number, _offsetY: number) => {}

  // ─── @State：子场景状态 ───
  /** 子星图 graph */
  @State childGraph: StarMapGraph | null = null
  /** 是否正在加载子星图 */
  @State isLoading: boolean = true
  /** 加载错误信息 */
  @State errorMessage: string = ''
  /** child scene 的路径（含 enterEmbed segment，由 EmbedScene 生成一次） */
  @State childScenePath: StarMapPathSegment[] = []
  /** resolver 解析出的最终 starmapId（用于加载子图和传给 child StarMapScene） */
  @State finalStarmapId: string = ''

  // ─── 生命周期 ───

  async aboutToAppear() {
    if (!this.bridge) {
      this.isLoading = false
      this.errorMessage = 'Bridge not available'
      return
    }
    if (!this.embed.targetStarmapId) {
      this.isLoading = false
      this.errorMessage = 'No target starmap'
      return
    }
    // 生成 childScenePath（由 EmbedScene 生成一次 enterEmbed segment）
    this.childScenePath = [...this.scenePath, { type: 'enterEmbed', instanceId: this.embed.instanceId, nodeId: null }]
    try {
      // 先用 resolver 解析路径，得到 finalStarmapId
      const resolveResult = await this.bridge.resolveStarMapPath(this.rootStarmapId, this.childScenePath)
      if (!resolveResult.success || !resolveResult.data || !resolveResult.data.finalStarmapId) {
        this.isLoading = false
        this.errorMessage = '解析子星图路径失败'
        return
      }
      this.finalStarmapId = resolveResult.data.finalStarmapId
      // 用 finalStarmapId 加载子图
      const result = await this.bridge.getStarMapGraph(this.finalStarmapId)
      if (result.success && result.data) {
        this.childGraph = result.data
      } else {
        this.errorMessage = '加载子星图失败'
      }
    } catch (err) {
      this.errorMessage = '加载子星图异常'
      hilog.error(0x0000, 'StarMapEmbedScene', `load error: ${JSON.stringify(err)}`)
    } finally {
      this.isLoading = false
    }
  }

  // ─── build ───

  build() {
    if (this.isLoading) {
      Column() {
        LoadingProgress()
          .width(24)
          .height(24)
      }
      .width('100%')
      .height('100%')
      .justifyContent(FlexAlign.Center)
      .alignItems(HorizontalAlign.Center)
    } else if (this.errorMessage) {
      Column() {
        Text(this.errorMessage)
          .fontSize(10)
          .fontColor('#999999')
      }
      .width('100%')
      .height('100%')
      .justifyContent(FlexAlign.Center)
      .alignItems(HorizontalAlign.Center)
    } else if (this.childGraph) {
      // 递归创建 child StarMapScene
      // childScenePath 由 EmbedScene 生成一次 enterEmbed segment
      StarMapScene({
        rootStarmapId: this.rootStarmapId,
        scenePath: this.childScenePath,
        starmapId: this.finalStarmapId,
        graph: $childGraph,
        // 全局相机原样透传：子 Scene 自己算 fitScale，没有可改的用户缩放（#818）
        cameraScale: this.cameraScale,
        cameraOffsetX: this.cameraOffsetX,
        cameraOffsetY: this.cameraOffsetY,
        onCameraChanged: this.onCameraChanged,
        // 空白拖动原样往下传，最终只改根全局相机（#818 复审）
        onCameraPanTo: this.onCameraPanTo,
        // 祖先累计比例原样往下传：第二层的累计 = camera × 第一层 fit × 第二层 fit（#818 复审）
        inheritedScale: this.inheritedScale,
        // 根视口与视觉焦点同样原样往下传（#820）：整棵递归树共用同一份根视口和同一个焦点，
        // 子 Scene 只按"自己这一层"参与 LOD 判定，不重算根视口、不改焦点。
        rootViewportWidth: this.rootViewportWidth,
        rootViewportHeight: this.rootViewportHeight,
        focusScenePath: this.focusScenePath,
        sceneDepth: this.sceneDepth + 1,
        bridge: this.bridge,
        themeConfig: this.themeConfig,
        motionConfig: this.motionConfig,
        motionProgress: this.motionProgress,
        gestureTracker: this.gestureTracker,
        // 选中和 Scene 注册都从根 Scene 一路传下来，子层不再自己存一份真相
        selectionState: this.selectionState,
        sceneRegistry: this.sceneRegistry,
        onCreateNode: (_path: StarMapPathSegment[], _starmapId: string, _title: string, _position: StarMapPoint) => {},
        onCreateEmbed: (_path: StarMapPathSegment[], _starmapId: string, _title: string, _position: StarMapPoint) => {},
        onCreateEdge: (_path: StarMapPathSegment[], _starmapId: string, _fromItemId: string, _toItemId: string) => {},
        onMoveNode: (_path: StarMapPathSegment[], _starmapId: string, _nodeId: string, _position: StarMapPoint) => {},
        onMoveEmbed: (_path: StarMapPathSegment[], _starmapId: string, _embedInstanceId: string, _position: StarMapPoint) => {},
      })
    } else {
      Column() {
        Text('空星图')
          .fontSize(10)
          .fontColor('#999999')
      }
      .width('100%')
      .height('100%')
      .justifyContent(FlexAlign.Center)
      .alignItems(HorizontalAlign.Center)
    }
  }
}
