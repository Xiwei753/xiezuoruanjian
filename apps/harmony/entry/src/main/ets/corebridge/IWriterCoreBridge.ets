import { EditorTransactionCause, EditorChangeStatsInput } from './dto/EditorDtos'
import { Project, Volume, ChapterMeta, ChapterContent, ChapterSaveReceipt, ProjectTree, ProjectStats, ProjectWritingLifetimeStats, WritingStats, SpeedCurve, CurrentWritingSpeed } from './dto/ProjectDtos'
import { AppSummary, ResultEnvelope, RecentEdit } from './dto/ResultEnvelope'
import { LocalSettings, SyncableSettings, ThemePaletteRecordDto, BuiltinThemeDto } from './dto/SettingsDtos'
import { StarMapMeta, StarMapGraph, StarMapNode, StarMapEdge, StarMapEmbed, StarMapPoint, StarMapTargetPath, StarMapPathSegment, StarMapResolvedPath, CreateStarmapChildEmbedResult, StarMapCrossLayerConnectionPlan, StarMapNodeUpdatePatch } from './dto/StarMapDtos'
import { SyncConfig, SyncSecrets, FullSyncResult, FullSyncDryRunResult, FullSyncDiagnosticsResult, SyncState } from './dto/SyncDtos'
import { SearchContext, SearchResultPage } from './dto/SearchDtos'
import { WritingActivitySegmentInput } from './dto/StatsDtos'

export interface IWriterCoreBridge {
  // App State 查询
  getAppState(): Promise<ResultEnvelope<AppSummary>>

  // Project
  listProjects(): Promise<ResultEnvelope<Project[]>>
  getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>>
  createProject(name: string): Promise<ResultEnvelope<Project>>
  renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>>
  deleteProject(projectId: string): Promise<ResultEnvelope<boolean>>
  getProjectStats(projectId: string): Promise<ResultEnvelope<ProjectStats>>
  getProjectWritingLifetimeStats(projectId: string): Promise<ResultEnvelope<ProjectWritingLifetimeStats>>

  // Volume
  listVolumes(projectId: string): Promise<ResultEnvelope<Volume[]>>
  createVolume(projectId: string, name: string): Promise<ResultEnvelope<Volume>>
  renameVolume(projectId: string, volumeId: string, newName: string): Promise<ResultEnvelope<boolean>>
  deleteVolume(projectId: string, volumeId: string): Promise<ResultEnvelope<boolean>>
  reorderVolumes(projectId: string, orderedVolumeIds: string[]): Promise<ResultEnvelope<boolean>>

  // Chapter
  listChapters(volumeId: string): Promise<ResultEnvelope<ChapterMeta[]>>
  loadChapter(chapterId: string): Promise<ResultEnvelope<ChapterContent>>
  saveChapter(chapterId: string, text: string): Promise<ResultEnvelope<ChapterSaveReceipt>>
  clearChapter(chapterId: string): Promise<ResultEnvelope<ChapterSaveReceipt>>
  createChapter(volumeId: string, name: string): Promise<ResultEnvelope<ChapterMeta>>
  renameChapter(chapterId: string, newName: string): Promise<ResultEnvelope<boolean>>
  deleteChapter(chapterId: string): Promise<ResultEnvelope<boolean>>
  reorderChapters(volumeId: string, orderedChapterIds: string[]): Promise<ResultEnvelope<boolean>>

  // StarMap
  listStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>>
  listRootStarMaps(): Promise<ResultEnvelope<StarMapMeta[]>>
  listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>>
  getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>>
  getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>>
  closeStarMapStore(starmapId: string): Promise<ResultEnvelope<boolean>>
  createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>>
  deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>>
  renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>>

  // StarMap Node/Edge/Embed mutations（领域数据写入，不属于显示层回流）
  // #984：节点更新接收完整 patch（title / content / position），只带要改的字段。
  updateStarMapNode(starmapId: string, nodeId: string, patch: StarMapNodeUpdatePatch): Promise<ResultEnvelope<StarMapNode>>
  addStarMapEdge(starmapId: string, from: StarMapTargetPath, to: StarMapTargetPath, kind: string, label: string | null): Promise<ResultEnvelope<StarMapEdge>>
  updateStarMapEmbed(starmapId: string, instanceId: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapEmbed>>
  addStarMapNode(starmapId: string, title: string, kind: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapNode>>
  addStarMapEmbed(starmapId: string, targetStarmapId: string, label: string, position: StarMapPoint): Promise<ResultEnvelope<StarMapEmbed>>
  createStarmapChildEmbed(hostStarmapId: string, title: string, position: StarMapPoint): Promise<ResultEnvelope<CreateStarmapChildEmbedResult>>
  resolveStarMapPath(rootStarmapId: string, segments: StarMapPathSegment[]): Promise<ResultEnvelope<StarMapResolvedPath>>
  // 跨层连线宿主规划：Core 决定 Edge/Link 落在哪张图（自环/不可解析/不同根会被拒绝）。
  planStarMapCrossLayerConnection(from: StarMapTargetPath, to: StarMapTargetPath): Promise<ResultEnvelope<StarMapCrossLayerConnectionPlan>>

  // Settings
  getLocalSettings(): Promise<ResultEnvelope<LocalSettings>>
  saveLocalSettings(settings: LocalSettings): Promise<ResultEnvelope<boolean>>
  getSyncableSettings(): Promise<ResultEnvelope<SyncableSettings>>
  saveSyncableSettings(settings: SyncableSettings): Promise<ResultEnvelope<boolean>>

  // Theme Palette
  listPaletteRecords(): Promise<ResultEnvelope<ThemePaletteRecordDto[]>>
  loadPaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<ThemePaletteRecordDto>>
  deletePaletteRecord(deviceId: string, fingerprint: string): Promise<ResultEnvelope<boolean>>
  listBuiltinThemes(): Promise<ResultEnvelope<BuiltinThemeDto[]>>

  // Sync（全量同步统一入口，Issue #630）
  // 一个全局 SyncConfig + 一份全局凭据，App target + 所有 Project target 一次同步。
  loadSyncConfig(): Promise<ResultEnvelope<SyncConfig>>
  saveSyncConfig(config: SyncConfig): Promise<ResultEnvelope<boolean>>
  loadSyncSecrets(): Promise<ResultEnvelope<SyncSecrets>>
  saveSyncSecrets(secrets: SyncSecrets): Promise<ResultEnvelope<boolean>>
  syncDryRun(): Promise<ResultEnvelope<FullSyncDryRunResult>>
  performSync(): Promise<ResultEnvelope<FullSyncResult>>
  syncDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>>

  // App target 同步状态（per-target 状态查询）
  loadAppSyncState(): Promise<ResultEnvelope<SyncState>>
  saveAppSyncState(state: SyncState): Promise<ResultEnvelope<boolean>>

  // Stats
  getWritingStats(): Promise<ResultEnvelope<WritingStats>>
  // Issue #829：带区间的汇总 / 速度曲线，供写作页状态栏读当日口径。
  getWritingStatsSummary(startDate: string, endDate: string): Promise<ResultEnvelope<WritingStats>>
  // Issue #829 评论10：「今天」由 Core 本地日历口径决定，端侧不传日期字符串。
  getTodayWritingStatsSummary(): Promise<ResultEnvelope<WritingStats>>
  getWritingSpeedCurve(startDate: string, endDate: string, bucketMinutes: number): Promise<ResultEnvelope<SpeedCurve>>
  // Issue #829：实时写作速度（最近 windowSeconds 秒），状态栏左段「字/时」由其折算。
  getCurrentWritingSpeed(windowSeconds: number): Promise<ResultEnvelope<CurrentWritingSpeed>>
  // Issue #829 评论5：按编辑事务上报写作统计。透传 cause + contentDelta，
  // source 分类由 Core 决定，端侧不要自己拼 source 字符串。
  recordEditorChangeStats(input: EditorChangeStatsInput): Promise<ResultEnvelope<boolean>>
  // Issue #981：上报已由平台单调时钟测量完成的前台写作切片。
  recordWritingActivitySegment(input: WritingActivitySegmentInput): Promise<ResultEnvelope<boolean>>
  calculateWordCount(text: string): Promise<number>
  getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>>

  // Global search（Issue #982）：分页、排序与命中范围全部由 Core 计算。
  globalSearchPage(
    query: string,
    scope: string,
    limit: number,
    cursor: string | null,
    context: SearchContext
  ): Promise<ResultEnvelope<SearchResultPage>>

  // Native Status
  getNativeLoadStatus(): Promise<ResultEnvelope<string>>
  isAiAvailable(): Promise<ResultEnvelope<boolean>>

  // Diagnostics
  setDiagnosticsConfig(enabled: boolean, verbose: boolean): Promise<ResultEnvelope<boolean>>
  flushDiagnostics(): Promise<ResultEnvelope<boolean>>
  clearDiagnostics(): Promise<ResultEnvelope<boolean>>
  exportDiagnostics(outputDir: string): Promise<ResultEnvelope<string>>
}
