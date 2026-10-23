// AppContinuationService — 当前任务接续（HarmonyOS Continuation）。
// Issue #988：接续状态按“页面快照”维护，而不是只有写作章节。
// - updateCurrentState()/updateCurrentScreen() 由真实前台页面写入；
// - getCurrentPayload() 由 ContinuationEntrypoint.onContinue 同步读取，源设备不在
//   onContinue 里等待 GitHub/附近设备传输；
// - restoreFromWant() 只恢复最近快照，目标页面按 screen/tabIndex 重建真实层级。
// 后台常驻的 Home 页面不能调用 clear() 清掉正在使用的写作/星图快照；需要清理时使用
// clearForScreen()，只有当前快照确实属于该页面才清。
//
// 这里不保存正文副本，也不保存 GitHub token/华为账号凭据。未保存正文由
// snapshotId/snapshotHash 描述，交付通道是应用接续快照或附近设备同步。

import {
  CONTINUATION_PAYLOAD_VERSION,
  ContinuationPayload,
  ContinuationScreen
} from './ContinuationPayload'
import { EditorViewportState } from '../../feature/editor/render/EditorViewportState'

const WANT_PARAM_KEY: string = 'continuationData'

export class AppContinuationService {
  private static instance: AppContinuationService
  private pendingPayload: ContinuationPayload | null = null
  // 进程内单调递增，避免旧页面迟到的快照覆盖较新的前台快照。
  private nextSequence: number = 1
  private readonly ownerId: string =
    `continuation-${Date.now()}-${Math.floor(Math.random() * 1000000)}`

  static getInstance(): AppContinuationService {
    if (!AppContinuationService.instance) {
      AppContinuationService.instance = new AppContinuationService()
    }
    return AppContinuationService.instance
  }

  // 页面状态变化时调用。payload 由页面层提供 screen 与对象 ID。
  updateCurrentState(payload: ContinuationPayload): void {
    const sanitized: ContinuationPayload = this.sanitizePayload(payload)
    sanitized.version = CONTINUATION_PAYLOAD_VERSION
    sanitized.snapshotOwner = this.ownerId
    sanitized.snapshotSequence = this.nextSequence++
    this.pendingPayload = sanitized
  }

  // 明确表达“当前页面快照”语义，供导航壳层和页面统一调用。
  updateCurrentScreen(payload: ContinuationPayload): void {
    this.updateCurrentState(payload)
  }

  async save(payload: ContinuationPayload): Promise<void> {
    this.updateCurrentState(payload)
  }

  getCurrentPayload(): ContinuationPayload | null {
    return this.pendingPayload
  }

  // 仅当当前快照属于指定页面时清理。后台 Home/星图列表的 aboutToAppear 不应
  // 清掉用户正在写作或浏览子星图的快照。
  clearForScreen(screen: ContinuationScreen): void {
    if (this.pendingPayload !== null && this.pendingPayload.screen === screen) {
      this.pendingPayload = null
    }
  }

  restoreFromWant(wantParam: Record<string, Object>): ContinuationPayload | null {
    const raw: Object | undefined = wantParam[WANT_PARAM_KEY]
    if (raw === undefined || raw === null) {
      return null
    }
    try {
      const json: string = raw as string
      const obj: Record<string, Object> = JSON.parse(json) as Record<string, Object>
      const restored: ContinuationPayload = this.sanitizePayload(this.fromObject(obj))
      restored.version = CONTINUATION_PAYLOAD_VERSION
      restored.snapshotOwner = this.ownerId
      restored.snapshotSequence = this.nextSequence++
      this.pendingPayload = restored
      return restored
    } catch (e) {
      return null
    }
  }

  clear(): void {
    this.pendingPayload = null
  }

  private sanitizePayload(payload: ContinuationPayload): ContinuationPayload {
    const result: ContinuationPayload = {}
    result.version = payload.version !== undefined ? payload.version : CONTINUATION_PAYLOAD_VERSION
    if (payload.snapshotOwner !== undefined) {
      result.snapshotOwner = payload.snapshotOwner
    }
    if (payload.snapshotSequence !== undefined) {
      result.snapshotSequence = payload.snapshotSequence
    }
    if (payload.screen !== undefined) {
      result.screen = payload.screen
    }
    if (payload.tabIndex !== undefined) {
      result.tabIndex = payload.tabIndex
    }
    if (payload.projectId !== undefined) {
      result.projectId = payload.projectId
    }
    if (payload.volumeId !== undefined) {
      result.volumeId = payload.volumeId
    }
    if (payload.chapterId !== undefined) {
      result.chapterId = payload.chapterId
    }
    if (payload.selection !== undefined) {
      result.selection = payload.selection.slice()
    }
    if (payload.cursor !== undefined) {
      result.cursor = payload.cursor
    }
    if (payload.viewport !== undefined) {
      result.viewport = payload.viewport
    }
    if (payload.contentHash !== undefined) {
      result.contentHash = payload.contentHash
    }
    if (payload.snapshotId !== undefined) {
      result.snapshotId = payload.snapshotId
    }
    if (payload.snapshotHash !== undefined) {
      result.snapshotHash = payload.snapshotHash
    }
    if (payload.sourceDeviceId !== undefined) {
      result.sourceDeviceId = payload.sourceDeviceId
    }
    if (payload.sourceNetworkId !== undefined) {
      result.sourceNetworkId = payload.sourceNetworkId
    }
    if (payload.starmapId !== undefined) {
      result.starmapId = payload.starmapId
    }
    if (payload.starmapTitle !== undefined) {
      result.starmapTitle = payload.starmapTitle
    }
    if (payload.parentStarmapId !== undefined) {
      result.parentStarmapId = payload.parentStarmapId
    }
    if (payload.parentStarmapTitle !== undefined) {
      result.parentStarmapTitle = payload.parentStarmapTitle
    }
    if (payload.parentPath !== undefined) {
      result.parentPath = payload.parentPath
    }
    if (payload.focusKind !== undefined) {
      result.focusKind = payload.focusKind
    }
    if (payload.focusId !== undefined) {
      result.focusId = payload.focusId
    }
    if (payload.focusScenePath !== undefined) {
      result.focusScenePath = payload.focusScenePath
    }
    if (payload.selectedNodeId !== undefined) {
      result.selectedNodeId = payload.selectedNodeId
    }
    if (payload.selectedKind !== undefined) {
      result.selectedKind = payload.selectedKind
    }
    if (payload.cameraScale !== undefined) {
      result.cameraScale = payload.cameraScale
    }
    if (payload.cameraOffsetX !== undefined) {
      result.cameraOffsetX = payload.cameraOffsetX
    }
    if (payload.cameraOffsetY !== undefined) {
      result.cameraOffsetY = payload.cameraOffsetY
    }
    if (payload.settingsSection !== undefined) {
      result.settingsSection = payload.settingsSection
    }
    if (payload.settingKey !== undefined) {
      result.settingKey = payload.settingKey
    }
    if (payload.statsPeriod !== undefined) {
      result.statsPeriod = payload.statsPeriod
    }
    if (payload.statsExpandedPanel !== undefined) {
      result.statsExpandedPanel = payload.statsExpandedPanel
    }
    if (payload.statsHeatmapYear !== undefined) {
      result.statsHeatmapYear = payload.statsHeatmapYear
    }
    if (payload.searchQuery !== undefined) {
      result.searchQuery = payload.searchQuery
    }
    if (payload.searchOriginScreen !== undefined) {
      result.searchOriginScreen = payload.searchOriginScreen
    }
    if (payload.searchProjectId !== undefined) {
      result.searchProjectId = payload.searchProjectId
    }
    if (payload.searchVolumeId !== undefined) {
      result.searchVolumeId = payload.searchVolumeId
    }
    if (payload.searchChapterId !== undefined) {
      result.searchChapterId = payload.searchChapterId
    }
    if (payload.searchStarmapId !== undefined) {
      result.searchStarmapId = payload.searchStarmapId
    }
    if (payload.searchSettingsSection !== undefined) {
      result.searchSettingsSection = payload.searchSettingsSection
    }
    if (payload.repoRemoteUrl !== undefined) {
      result.repoRemoteUrl = payload.repoRemoteUrl
    }
    if (payload.repoBranch !== undefined) {
      result.repoBranch = payload.repoBranch
    }
    return result
  }

  private fromObject(obj: Record<string, Object>): ContinuationPayload {
    const result: ContinuationPayload = {}
    if (obj['version'] !== undefined) {
      result.version = obj['version'] as number
    }
    if (obj['snapshotOwner'] !== undefined) {
      result.snapshotOwner = obj['snapshotOwner'] as string
    }
    if (obj['snapshotSequence'] !== undefined) {
      result.snapshotSequence = obj['snapshotSequence'] as number
    }
    if (obj['screen'] !== undefined) {
      result.screen = obj['screen'] as ContinuationScreen
    }
    if (obj['tabIndex'] !== undefined) {
      result.tabIndex = obj['tabIndex'] as number
    }
    if (obj['projectId'] !== undefined) {
      result.projectId = obj['projectId'] as string
    }
    if (obj['volumeId'] !== undefined) {
      result.volumeId = obj['volumeId'] as string
    }
    if (obj['chapterId'] !== undefined) {
      result.chapterId = obj['chapterId'] as string
    }
    if (obj['selection'] !== undefined) {
      result.selection = obj['selection'] as number[]
    }
    if (obj['cursor'] !== undefined) {
      result.cursor = obj['cursor'] as number
    }
    if (obj['viewport'] !== undefined) {
      result.viewport = EditorViewportState.fromObject(obj['viewport'] as Record<string, Object>)
    }
    if (obj['contentHash'] !== undefined) {
      result.contentHash = obj['contentHash'] as string
    }
    if (obj['snapshotId'] !== undefined) {
      result.snapshotId = obj['snapshotId'] as string
    }
    if (obj['snapshotHash'] !== undefined) {
      result.snapshotHash = obj['snapshotHash'] as string
    }
    if (obj['sourceDeviceId'] !== undefined) {
      result.sourceDeviceId = obj['sourceDeviceId'] as string
    }
    if (obj['sourceNetworkId'] !== undefined) {
      result.sourceNetworkId = obj['sourceNetworkId'] as string
    }
    if (obj['starmapId'] !== undefined) {
      result.starmapId = obj['starmapId'] as string
    }
    if (obj['starmapTitle'] !== undefined) {
      result.starmapTitle = obj['starmapTitle'] as string
    }
    if (obj['parentStarmapId'] !== undefined) {
      result.parentStarmapId = obj['parentStarmapId'] as string
    }
    if (obj['parentStarmapTitle'] !== undefined) {
      result.parentStarmapTitle = obj['parentStarmapTitle'] as string
    }
    if (obj['parentPath'] !== undefined) {
      result.parentPath = obj['parentPath'] as string
    }
    if (obj['focusKind'] !== undefined) {
      result.focusKind = obj['focusKind'] as string
    }
    if (obj['focusId'] !== undefined) {
      result.focusId = obj['focusId'] as string
    }
    if (obj['focusScenePath'] !== undefined) {
      result.focusScenePath = obj['focusScenePath'] as string
    }
    if (obj['selectedNodeId'] !== undefined) {
      result.selectedNodeId = obj['selectedNodeId'] as string
    }
    if (obj['selectedKind'] !== undefined) {
      result.selectedKind = obj['selectedKind'] as string
    }
    if (obj['cameraScale'] !== undefined) {
      result.cameraScale = obj['cameraScale'] as number
    }
    if (obj['cameraOffsetX'] !== undefined) {
      result.cameraOffsetX = obj['cameraOffsetX'] as number
    }
    if (obj['cameraOffsetY'] !== undefined) {
      result.cameraOffsetY = obj['cameraOffsetY'] as number
    }
    if (obj['settingsSection'] !== undefined) {
      result.settingsSection = obj['settingsSection'] as string
    }
    if (obj['settingKey'] !== undefined) {
      result.settingKey = obj['settingKey'] as string
    }
    if (obj['statsPeriod'] !== undefined) {
      result.statsPeriod = obj['statsPeriod'] as string
    }
    if (obj['statsExpandedPanel'] !== undefined) {
      result.statsExpandedPanel = obj['statsExpandedPanel'] as string
    }
    if (obj['statsHeatmapYear'] !== undefined) {
      result.statsHeatmapYear = obj['statsHeatmapYear'] as number
    }
    if (obj['searchQuery'] !== undefined) {
      result.searchQuery = obj['searchQuery'] as string
    }
    if (obj['searchOriginScreen'] !== undefined) {
      result.searchOriginScreen = obj['searchOriginScreen'] as string
    }
    if (obj['searchProjectId'] !== undefined) {
      result.searchProjectId = obj['searchProjectId'] as string
    }
    if (obj['searchVolumeId'] !== undefined) {
      result.searchVolumeId = obj['searchVolumeId'] as string
    }
    if (obj['searchChapterId'] !== undefined) {
      result.searchChapterId = obj['searchChapterId'] as string
    }
    if (obj['searchStarmapId'] !== undefined) {
      result.searchStarmapId = obj['searchStarmapId'] as string
    }
    if (obj['searchSettingsSection'] !== undefined) {
      result.searchSettingsSection = obj['searchSettingsSection'] as string
    }
    if (obj['repoRemoteUrl'] !== undefined) {
      result.repoRemoteUrl = obj['repoRemoteUrl'] as string
    }
    if (obj['repoBranch'] !== undefined) {
      result.repoBranch = obj['repoBranch'] as string
    }
    return result
  }

  serializeForWant(payload: ContinuationPayload): string {
    return JSON.stringify(this.sanitizePayload(payload))
  }
}
