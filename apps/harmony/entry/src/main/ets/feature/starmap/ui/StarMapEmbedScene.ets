// ─── StarMapEmbedScene.ets ───
// Embed 内部内容区的递归桥梁组件。
// StarMapScene 渲染 Embed 时，Embed 的 innerContent 区域使用 StarMapEmbedScene，
// StarMapEmbedScene 内部递归创建 child StarMapScene。
//
// 职责：
// - 加载子星图 graph（通过 bridge.getStarMapGraph）
// - 维护子场景的 viewport 状态（zoomScale、offsetX、offsetY、selectedNodeId）
// - 将子 StarMapScene 嵌入 Embed 的 innerContent 区域

import hilog from '@ohos.hilog'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import {
  StarMapGraph, StarMapEmbed, StarMapPoint, StarMapPathSegment
} from '../../../corebridge/dto/StarMapDtos'
import { ThemeConfig } from '../../../ui/theme/HarmonyThemeAdapter'
import { PlatformMotionConfig } from '../platform/StarMapMotion'
import { StarMapGestureStateTracker } from '../platform/StarMapGestureState'
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

  // ─── @State：子场景状态 ───
  /** 子星图 graph */
  @State childGraph: StarMapGraph | null = null
  /** 子场景缩放 */
  @State childZoomScale: number = 1
  /** 子场景 X 偏移 */
  @State childOffsetX: number = 0
  /** 子场景 Y 偏移 */
  @State childOffsetY: number = 0
  /** 子场景选中对象 ID */
  @State childSelectedNodeId: string = ''
  /** 是否正在加载子星图 */
  @State isLoading: boolean = true
  /** 加载错误信息 */
  @State errorMessage: string = ''

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
    try {
      const result = await this.bridge.getStarMapGraph(this.embed.targetStarmapId)
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
      StarMapScene({
        rootStarmapId: this.rootStarmapId,
        scenePath: [...this.scenePath, { type: 'enterEmbed', instanceId: this.embed.instanceId, nodeId: null }],
        starmapId: this.embed.targetStarmapId,
        graph: $childGraph,
        zoomScale: $childZoomScale,
        offsetX: $childOffsetX,
        offsetY: $childOffsetY,
        sceneDepth: this.sceneDepth + 1,
        selectedNodeId: $childSelectedNodeId,
        bridge: this.bridge,
        themeConfig: this.themeConfig,
        motionConfig: this.motionConfig,
        motionProgress: this.motionProgress,
        gestureTracker: this.gestureTracker,
        onSelectItem: (_path: StarMapPathSegment[], _itemId: string) => {},
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
