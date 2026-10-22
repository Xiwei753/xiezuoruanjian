import { IWriterCoreBridge } from './IWriterCoreBridge'
import { EditorTransactionCause, EditorChangeStatsInput } from './dto/EditorDtos'
import { Project, Volume, ChapterMeta, ChapterContent, ChapterSaveReceipt, ProjectTree, ProjectStats, ProjectWritingLifetimeStats, WritingStats, SpeedCurve, CurrentWritingSpeed } from './dto/ProjectDtos'
import { AppSummary, ResultEnvelope, RecentEdit } from './dto/ResultEnvelope'
import { LocalSettings, SyncableSettings, ThemePaletteRecordDto, BuiltinThemeDto } from './dto/SettingsDtos'
import { StarMapMeta, StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed, StarMapPoint, StarMapTargetPath, StarMapPathSegment, StarMapResolvedPath, CreateStarmapChildEmbedResult, StarMapCrossLayerConnectionPlan } from './dto/StarMapDtos'
import { SyncConfig, SyncSecrets, FullSyncResult, FullSyncDryRunResult, FullSyncDiagnosticsResult, SyncState } from './dto/SyncDtos'
import { SearchContext, SearchResultPage } from './dto/SearchDtos'
import { WritingActivitySegmentInput } from './dto/StatsDtos'
import { NativeCoreModule } from './native/NativeCoreModule'
import { NativeAppStateBridge } from './native/NativeAppStateBridge'
import { NativeProjectBridge } from './native/NativeProjectBridge'
import { NativeChapterBridge } from './native/NativeChapterBridge'
import { NativeSettingsBridge } from './native/NativeSettingsBridge'
import { NativeSyncBridge } from './native/NativeSyncBridge'
import { NativeStatsBridge } from './native/NativeStatsBridge'
import { NativeStarMapBridge } from './native/NativeStarMapBridge'
import { NativeSearchBridge } from './native/NativeSearchBridge'
import { NativeDiagnosticsBridge } from './native/NativeDiagnosticsBridge'
import { NativeInitParams } from '../platform/init/NativeInitParams'

export class NativeWriterCoreBridge implements IWriterCoreBridge {
  private static instance: NativeWriterCoreBridge
  private isInitialized: boolean = false
  private appDataRoot: string = ''
  private lastInitErrorCode: number = 0

  // ── 公共模块 & 分域 Bridge ──
  private core = new NativeCoreModule()
  private appStateBridge = new NativeAppStateBridge(this.core)
  private projectBridge = new NativeProjectBridge(this.core)
  private chapterBridge = new NativeChapterBridge(this.core)
  private settingsBridge = new NativeSettingsBridge(this.core)
  private syncBridge = new NativeSyncBridge(this.core)
  private statsBridge = new NativeStatsBridge(this.core)
  private starMapBridge = new NativeStarMapBridge(this.core)
  private searchBridge = new NativeSearchBridge(this.core)
  private diagnosticsBridge = new NativeDiagnosticsBridge(this.core)

  static getInstance(): NativeWriterCoreBridge {
    if (!NativeWriterCoreBridge.instance) {
      NativeWriterCoreBridge.instance = new NativeWriterCoreBridge()
    }
    return NativeWriterCoreBridge.instance
  }

  async initialize(params: NativeInitParams): Promise<boolean> {
    if (this.isInitialized) {
      return true
    }
    try {
      // 网络状态为三态：查询失败（unknown）传 -1，不得伪造成在线。
      const isConnected: number = params.networkKnown ? (params.isConnected ? 1 : 0) : -1
      const isMetered: number = params.networkKnown ? (params.isMetered ? 1 : 0) : -1

      const result: number = this.core.getNativeModule().nativeInit(
        params.appDataRoot,
        params.filesDir,   // PlatformInit.app_data_dir，与 appDataRoot 同一数据根
        params.cacheDir,   // 真实 UIAbilityContext.cacheDir
        params.deviceId,
        params.appVersion,
        params.locale,
        params.timezone,
        isConnected,
        isMetered
      ) as number
      if (result === 0) {
        this.isInitialized = true
        this.appDataRoot = params.appDataRoot
        this.statsBridge.setInitialized(true)
        return true
      }
      this.lastInitErrorCode = result
      console.error(`nativeInit failed with code: ${result}, path: ${params.appDataRoot}`)
      return false
    } catch (e) {
      this.lastInitErrorCode = -999
      this.core.markNativeUnavailable()
      console.error(`nativeInit exception: ${e}`)
      return false
    }
  }

  /**
   * 用平台最新的网络查询结果更新 Core 缓存的 NetworkState。
   * 三态：known ? 1/0 : -1；未知时 Core 清除缓存状态，不保留伪造的在线快照。
   * 由系统网络事件或同步操作前的查询调用。
   */
  updateNetworkState(known: boolean, isConnected: boolean, isMetered: boolean): number {
    try {
      const connected: number = known ? (isConnected ? 1 : 0) : -1
      const metered: number = known ? (isMetered ? 1 : 0) : -1
      return this.core.getNativeModule().nativeUpdateNetworkState(connected, metered) as number
    } catch (e) {
      console.error(`nativeUpdateNetworkState exception: ${e}`)
      return -999
    }
  }

  // ── Diagnostics Init（Issue #670 评论 5652119660 修复 2d / #911） ──
  // 把目录/设备/版本/locale/timezone 等信息从 ArkTS 传进 Rust，
  // 启动统一日志后端。session_id 在 Rust 里生成；enabled/verbose 由 Rust
  // 从 appDataRoot 下的 settings.local.json 读取（不再写死 true）。
  // 必须在第一批诊断事件之前调用，否则事件会被丢弃。
  // 返回 0 表示成功，非 0 表示失败（-1 参数为 null，-2 无效 UTF-8，-3 设置读取失败）。
  initDiagnostics(
    logDir: string,
    appDataRoot: string,
    deviceId: string,
    appVersion: string,
    buildKey: string,
    locale: string,
    timezone: string
  ): number {
    try {
      const result: number = this.core.getNativeModule().nativeInitDiagnostics(
        logDir, appDataRoot, deviceId, appVersion, buildKey, locale, timezone
      ) as number
      if (result !== 0) {
        console.error(`nativeInitDiagnostics failed with code: ${result}`)
      }
      return result
    } catch (e) {
      this.core.markNativeUnavailable()
      console.error(`nativeInitDiagnostics exception: ${e}`)
      return -999
    }
  }

  getLastInitErrorCode(): number {
    return this.lastInitErrorCode
  }

  async getLastError(): Promise<string> {
    try {
      const err: string = this.core.getNativeModule().nativeGetLastError() as string
      return err
    } catch (e) {
      return String(e)
    }
  }

  // ── App State ──

  async getAppState(): Promise<ResultEnvelope<AppSummary>> {
    return this.appStateBridge.getAppState()
  }

  // ── Project ──

  async listProjects(): Promise<ResultEnvelope<Project[]>> {
    return this.projectBridge.listProjects()
  }

  async getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>> {
    return this.projectBridge.getProjectTree(projectId)
  }

  async createProject(name: string): Promise<ResultEnvelope<Project>> {
    return this.projectBridge.createProject(name)
  }

  async renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.renameProject(projectId, newName)
  }

  async deleteProject(projectId: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.deleteProject(projectId)
  }

  async getProjectStats(projectId: string): Promise<ResultEnvelope<ProjectStats>> {
    return this.projectBridge.getProjectStats(projectId)
  }

  async getProjectWritingLifetimeStats(projectId: string): Promise<ResultEnvelope<ProjectWritingLifetimeStats>> {
    return this.projectBridge.getProjectWritingLifetimeStats(projectId)
  }

  // ── Volume ──

  async listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>> {
    return this.projectBridge.listVolumes(projectId)
  }

  async createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>> {
    return this.projectBridge.createVolume(projectId, name)
  }

  async renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.renameVolume(projectId, volumeId, newName)
  }

  async deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.deleteVolume(projectId, volumeId)
  }

  async reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>> {
    return this.projectBridge.reorderVolumes(projectId, orderedVolumeIds)
  }

  // ── Chapter ──

  async listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>> {
    return this.chapterBridge.listChapters(volumeId)
  }

  async loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterContent>> {
    return this.chapterBridge.loadChapter(chapterId)
  }

  async saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    return this.chapterBridge.saveChapter(chapterId, text)
  }

  async clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>> {
    return this.chapterBridge.clearChapter(chapterId)
  }

  async createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>> {
    return this.chapterBridge.createChapter(volumeId, name)
  }

  async renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>> {
    return this.chapterBridge.renameChapter(chapterId, newName)
  }

  async deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>> {
    return this.chapterBridge.deleteChapter(chapterId)
  }

  async reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>> {
    return this.chapterBridge.reorderChapters(volumeId, orderedChapterIds)
  }

  // ── StarMap ──

  async listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    return this.starMapBridge.listStarMaps()
  }

  async listRootStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>> {
    return this.starMapBridge.listRootStarMaps()
  }

  async listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>> {
    return this.starMapBridge.listStarMapsForProject(projectId)
  }

  async getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>> {
    return this.starMapBridge.getStarMap(starmapId)
  }

  async getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>> {
    return this.starMapBridge.getStarMapGraph(starmapId)
  }

  async createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>> {
    return this.starMapBridge.createStarMap(title, description)
  }

  async deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>> {
    return this.starMapBridge.deleteStarMap(starmapId)
  }

  async renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>> {
    return this.starMapBridge.renameStarMap(starmapId, newTitle)
  }

  // StarMap Node/Edge/Embed mutations
  async updateStarMapNode(starmapId: string, nodeId: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapNode>> {
    return this.starMapBridge.updateStarMapNode(starmapId, nodeId, position)
  }

  async addStarMapEdge(starmapId: string, from: StarMapTargetPath, to: StarMapTargetPath, kind: string, label: string | null): Promise<ResultEnvelope<StarMapEdge>> {
    return this.starMapBridge.addStarMapEdge(starmapId, from, to, kind, label)
  }

  async updateStarMapEmbed(starmapId: string, instanceId: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapEmbed>> {
    return this.starMapBridge.updateStarMapEmbed(starmapId, instanceId, position)
  }

  async addStarMapNode(starmapId: string, title: string, kind: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapNode>> {
    return this.starMapBridge.addStarMapNode(starmapId, title, kind, position)
  }

  async addStarMapEmbed(starmapId: string, targetStarmapId: string, label: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapEmbed>> {
    return this.starMapBridge.addStarMapEmbed(starmapId, targetStarmapId, label, position)
  }

  async createStarmapChildEmbed(hostStarmapId: string, title: string, position: StarMapPoint): Promise<ResultEnvelope<CreateStarmapChildEmbedResult>> {
    return this.starMapBridge.createStarmapChildEmbed(hostStarmapId, title, position)
  }

  async resolveStarMapPath(rootStarmapId: string, segments: StarMapPathSegment[]): Promise<ResultEnvelope<StarMapResolvedPath>> {
    return this.starMapBridge.resolveStarMapPath(rootStarmapId, segments)
  }

  async planStarMapCrossLayerConnection(from: StarMapTargetPath, to: StarMapTargetPath): Promise<ResultEnvelope<StarMapCrossLayerConnectionPlan>> {
    return this.starMapBridge.planStarMapCrossLayerConnection(from, to)
  }

  // ── Settings ──

  async getLocalSettings(): Promise<ResultEnvelope<LocalSettings>> {
    return this.settingsBridge.getLocalSettings()
  }

  async saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>> {
    return this.settingsBridge.saveLocalSettings(settings)
  }

  async getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>> {
    return this.settingsBridge.getSyncableSettings()
  }

  async saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>> {
    return this.settingsBridge.saveSyncableSettings(settings)
  }

  async listPaletteRecords(): Promise<ResultEnvelope<ThemePaletteRecordDto[]>> {
    return this.settingsBridge.listPaletteRecords()
  }

  async loadPaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<ThemePaletteRecordDto>> {
    return this.settingsBridge.loadPaletteRecord(deviceId, fingerprint)
  }

  async deletePaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<boolean>> {
    return this.settingsBridge.deletePaletteRecord(deviceId, fingerprint)
  }

  async listBuiltinThemes(): Promise<ResultEnvelope<BuiltinThemeDto[]>> {
    return this.settingsBridge.listBuiltinThemes()
  }

  // ── Sync（全量同步统一入口，Issue #630） ──
  // 一个全局 SyncConfig + 一份全局凭据，App target + 所有 Project target 一次同步。

  async loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>> {
    return this.syncBridge.loadSyncConfig()
  }

  async saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>> {
    return this.syncBridge.saveSyncConfig(config)
  }

  async loadSyncSecrets(): Promise<ResultEnvelope<SyncSecrets>> {
    return this.syncBridge.loadSyncSecrets()
  }

  async saveSyncSecrets(secrets: SyncSecrets): Promise<ResultEnvelope<boolean>> {
    return this.syncBridge.saveSyncSecrets(secrets)
  }

  async syncDryRun(): Promise<ResultEnvelope<FullSyncDryRunResult>> {
    return this.syncBridge.syncDryRun()
  }

  async performSync(): Promise<ResultEnvelope<FullSyncResult>> {
    return this.syncBridge.performSync()
  }

  async syncDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>> {
    return this.syncBridge.syncDiagnostics()
  }

  // ── App target 同步状态（per-target 状态查询） ──

  async loadAppSyncState(): Promise<ResultEnvelope<SyncState>> {
    return this.syncBridge.loadAppSyncState()
  }

  async saveAppSyncState(state: SyncState): Promise<ResultEnvelope<boolean>> {
    return this.syncBridge.saveAppSyncState(state)
  }

  // ── Stats ──

  async getWritingStats(): Promise<ResultEnvelope<WritingStats>> {
    return this.statsBridge.getWritingStats()
  }

  async getWritingStatsSummary(startDate: string, endDate: string): Promise<ResultEnvelope<WritingStats>> {
    return this.statsBridge.getWritingStatsSummary(startDate, endDate)
  }

  async getTodayWritingStatsSummary(): Promise<ResultEnvelope<WritingStats>> {
    return this.statsBridge.getTodayWritingStatsSummary()
  }

  async getWritingSpeedCurve(startDate: string, endDate: string, bucketMinutes: number): Promise<ResultEnvelope<SpeedCurve>> {
    return this.statsBridge.getWritingSpeedCurve(startDate, endDate, bucketMinutes)
  }

  async getCurrentWritingSpeed(windowSeconds: number): Promise<ResultEnvelope<CurrentWritingSpeed>> {
    return this.statsBridge.getCurrentWritingSpeed(windowSeconds)
  }

  async recordEditorChangeStats(input: EditorChangeStatsInput): Promise<ResultEnvelope<boolean>> {
    return this.statsBridge.recordEditorChangeStats(input)
  }

  async recordWritingActivitySegment(input: WritingActivitySegmentInput): Promise<ResultEnvelope<boolean>> {
    return this.statsBridge.recordWritingActivitySegment(input)
  }

  async calculateWordCount(text: string): Promise<number> {
    return this.statsBridge.calculateWordCount(text)
  }

  async getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>> {
    return this.statsBridge.getRecentEdits()
  }

  // ── Global search ──

  async globalSearchPage(
    query: string,
    scope: string,
    limit: number,
    cursor: string | null,
    context: SearchContext
  ): Promise<ResultEnvelope<SearchResultPage>> {
    return this.searchBridge.globalSearchPage(query, scope, limit, cursor, context)
  }

  // ── Native Status ──

  async getNativeLoadStatus(): Promise<ResultEnvelope<string>> {
    try {
      const status: string = this.core.getNativeModule().nativeGetLoadStatus() as string
      const result: ResultEnvelope<string> = {
        success: true,
        data: status }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
    }
  }

  async isAiAvailable(): Promise<ResultEnvelope<boolean>> {
    try {
      const available: number = this.core.getNativeModule().nativeIsAiAvailable() as number
      const result: ResultEnvelope<boolean> = {
        success: true,
        data: available === 1 }
      return result
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
    }
  }

  // ── Diagnostics ──

  async setDiagnosticsConfig(enabled: boolean, verbose: boolean): Promise<ResultEnvelope<boolean>> {
    try {
      const result: number = this.diagnosticsBridge.setDiagnosticsConfig(enabled, verbose)
      const envelope: ResultEnvelope<boolean> = {
        success: result === 0,
        data: result === 0
      }
      return envelope
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
    }
  }

  async flushDiagnostics(): Promise<ResultEnvelope<boolean>> {
    try {
      const result: number = this.diagnosticsBridge.flushDiagnostics()
      const envelope: ResultEnvelope<boolean> = {
        success: result === 0,
        data: result === 0
      }
      return envelope
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
    }
  }

  async clearDiagnostics(): Promise<ResultEnvelope<boolean>> {
    try {
      const result: number = this.diagnosticsBridge.clearDiagnostics()
      const envelope: ResultEnvelope<boolean> = {
        success: result === 0,
        data: result === 0
      }
      return envelope
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
    }
  }

  async exportDiagnostics(outputDir: string): Promise<ResultEnvelope<string>> {
    try {
      // 不带平台附件的基础导出 — 平台附件收集由 HarmonyDiagnosticsExporter 负责。
      const result: string = this.diagnosticsBridge.exportDiagnostics(outputDir, '[]')
      // 检查返回值：成功是 zip 文件路径，失败是 JSON 错误 envelope（以 { 开头）
      if (result.startsWith('{')) {
        const errorEnvelope: ResultEnvelope<string> = JSON.parse(result) as ResultEnvelope<string>
        return errorEnvelope
      }
      const envelope: ResultEnvelope<string> = {
        success: true,
        data: result
      }
      return envelope
    } catch (e) {
      return { success: false, errorCode: 'NATIVE_ERROR', userMessage: String(e)}
    }
  }
}
