// Embed 内部真实子图投影的递归桥梁。
//
// 父画布把入口圆壳内部交给本组件：加载入口 route 终点星图，并在圆内实例化一份
// 真正的子 StarMapScene（节点卡片、文字、连线和更深入口都照常画）。
// 这不是缩略图、不是小圆点、不是按倍率切换的 LOD 档位——就是子图按当前相机
// 比例缩小后的真实画面；看不清只是自然结果。
//
// 循环引用：祖先链上的 starmap 允许再投影一层（A→A 或 A→B→A 进入后仍能看见
// 下一层真实内容）；第二层再遇到同一祖先就停止实例化，只保留入口本身。
// 因而循环不会变成无限组件树，路径本身也不会被拒绝。

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
  @Prop rootStarmapId: string = ''
  /** 父画布自己的 route（不含当前入口这一段的起点） */
  @Prop scenePath: StarMapPathSegment[] = []
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
  @Prop sceneDepth: number = 0

  @Prop themeConfig: ThemeConfig = {
    mode: 'light',
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
    fontSize: 14, fontFamily: 'HarmonyOS Sans', lineHeight: 1.5
  }
  @Prop motionConfig: PlatformMotionConfig = {
    enabled: true, reduceMotion: false,
    idleWobbleEnabled: true, idleAmplitudeVp: 2.0,
    idlePeriodMs: 4200, dragLiftScale: 1.04,
    settleDurationMs: 220
  }
  @Prop motionProgress: number = 0

  @Prop cameraScale: number = 1
  @Prop cameraOffsetX: number = 0
  @Prop cameraOffsetY: number = 0
  @Prop inheritedScale: number = 1

  bridge: IWriterCoreBridge | null = null
  gestureTracker: StarMapGestureStateTracker = new StarMapGestureStateTracker()
  @State selectionState: StarMapSelectionState = new StarMapSelectionState()
  sceneRegistry: StarMapSceneRegistry | null = null
  onCameraChanged: (scale: number, offsetX: number, offsetY: number) => void =
    (_scale: number, _offsetX: number, _offsetY: number) => {}
  onCameraPanTo: (offsetX: number, offsetY: number) => void =
    (_offsetX: number, _offsetY: number) => {}
  onEnterEmbed: (path: StarMapPathSegment[]) => void = (_path: StarMapPathSegment[]) => {}

  /** 当前 route 已出现的 starmapId（含根，允许重复）。 */
  @Prop ancestorStarmapIds: string[] = []
  /** 是否还允许把“已出现在祖先链上的目标”再投影一层。 */
  @Prop allowCycleProjection: boolean = true

  @State childGraph: StarMapGraph | null = null
  @State isLoading: boolean = true
  @State errorMessage: string = ''
  @State childScenePath: StarMapPathSegment[] = []
  @State finalStarmapId: string = ''
  @State childAncestorStarmapIds: string[] = []
  @State childAllowCycleProjection: boolean = true
  @State isCycleBoundary: boolean = false

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
    const childPath: StarMapPathSegment[] = [
      ...this.scenePath,
      { type: 'enterEmbed', instanceId: this.embed.instanceId, nodeId: null }
    ]
    this.childScenePath = childPath
    try {
      const resolveResult = await this.bridge.resolveStarMapPath(this.rootStarmapId, childPath)
      if (!resolveResult.success || !resolveResult.data || !resolveResult.data.finalStarmapId) {
        this.isLoading = false
        this.errorMessage = '解析子星图路径失败'
        return
      }
      const finalId: string = resolveResult.data.finalStarmapId
      this.finalStarmapId = finalId
      const repeated: boolean = this.ancestorStarmapIds.includes(finalId)
      if (repeated && !this.allowCycleProjection) {
        // 循环已经投影过一层：保留入口圆壳，不再实例化更深 Scene。
        this.isCycleBoundary = true
        this.isLoading = false
        return
      }
      this.childAncestorStarmapIds = [...this.ancestorStarmapIds, finalId]
      this.childAllowCycleProjection = repeated ? false : this.allowCycleProjection
      const result = await this.bridge.getStarMapGraph(finalId)
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
    } else if (this.isCycleBoundary) {
      Column() {
      }
      .width('100%')
      .height('100%')
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
      StarMapScene({
        rootStarmapId: this.rootStarmapId,
        scenePath: this.childScenePath,
        starmapId: this.finalStarmapId,
        graph: $childGraph,
        cameraScale: this.cameraScale,
        cameraOffsetX: this.cameraOffsetX,
        cameraOffsetY: this.cameraOffsetY,
        onCameraChanged: this.onCameraChanged,
        onCameraPanTo: this.onCameraPanTo,
        inheritedScale: this.inheritedScale,
        sceneDepth: this.sceneDepth + 1,
        ancestorStarmapIds: this.childAncestorStarmapIds,
        allowCycleProjection: this.childAllowCycleProjection,
        onEnterEmbed: this.onEnterEmbed,
        bridge: this.bridge,
        themeConfig: this.themeConfig,
        motionConfig: this.motionConfig,
        motionProgress: this.motionProgress,
        gestureTracker: this.gestureTracker,
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
