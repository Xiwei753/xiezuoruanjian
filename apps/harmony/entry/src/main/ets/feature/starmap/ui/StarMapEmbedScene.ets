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
  /** 全局相机比例：原样往下传，子 Scene 只是相机里的内容（#818） */
  cameraScale: number = 1
  /** 全局相机 X 偏移：原样往下传 */
  cameraOffsetX: number = 0
  /** 全局相机 Y 偏移：原样往下传 */
  cameraOffsetY: number = 0
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
        // 祖先累计比例原样往下传：第二层的累计 = camera × 第一层 fit × 第二层 fit（#818 复审）
        inheritedScale: this.inheritedScale,
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
