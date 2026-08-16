import { EditorVisualTransaction, EditorTransactionCause } from './dto/EditorDtos'
import { ScreenPolicy, ScreenRole } from './dto/PlatformDtos'
import { Project, Volume, ChapterMeta, ChapterContent, ChapterSaveReceipt, ProjectTree, WritingStats } from './dto/ProjectDtos'
import { AppSummary, ResultEnvelope, RecentEdit } from './dto/ResultEnvelope'
import { LocalSettings, SyncableSettings, ThemePaletteRecordDto, BuiltinThemeDto } from './dto/SettingsDtos'
import { StarMapMeta, StarMapGraph, StarMapLayout, StarMapEdgeRender, StarMapMotionPolicy } from './dto/StarMapDtos'
import { SyncConfig, FullSyncResult, FullSyncDryRunResult, FullSyncDiagnosticsResult, SyncState } from './dto/SyncDtos'

export interface IWriterCoreBridge {
  // App State 查询
  listAppSummaries(): Promise<ResultEnvelope<AppSummary[]>>
  getAppState(): Promise<ResultEnvelope<AppSummary>>

  // Project
  listProjects(): Promise<ResultEnvelope<Project[]>>
  getProjectTree(projectId: string): Promise<ResultEnvelope<ProjectTree>>
  createProject(name: string): Promise<ResultEnvelope<Project>>
  renameProject(projectId: string, newName: string): Promise<ResultEnvelope<boolean>>
  deleteProject(projectId: string): Promise<ResultEnvelope<boolean>>
  getProjectStats(projectId: string): Promise<ResultEnvelope<WritingStats>>

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
  listStarMapsForProject(projectId: string): Promise<ResultEnvelope<StarMapMeta[]>>
  getStarMap(starmapId: string): Promise<ResultEnvelope<StarMapMeta>>
  getStarMapGraph(starmapId: string): Promise<ResultEnvelope<StarMapGraph>>
  createStarMap(title: string, description: string): Promise<ResultEnvelope<StarMapMeta>>
  deleteStarMap(starmapId: string): Promise<ResultEnvelope<boolean>>
  renameStarMap(starmapId: string, newTitle: string): Promise<ResultEnvelope<boolean>>

  // StarMap Layout & Motion
  saveStarMapLayout(starmapId: string, layout: StarMapLayout): Promise<ResultEnvelope<boolean>>
  getStarMapLayout(starmapId: string): Promise<ResultEnvelope<StarMapLayout>>
  getStarMapMotionPolicy(): Promise<ResultEnvelope<StarMapMotionPolicy>>
  computeStarMapEdgeRenders(graph: StarMapGraph): Promise<ResultEnvelope<StarMapEdgeRender[]>>

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
  syncDryRun(): Promise<ResultEnvelope<FullSyncDryRunResult>>
  performSync(): Promise<ResultEnvelope<FullSyncResult>>
  syncDiagnostics(): Promise<ResultEnvelope<FullSyncDiagnosticsResult>>

  // App target 同步状态（per-target 状态查询）
  loadAppSyncState(): Promise<ResultEnvelope<SyncState>>
  saveAppSyncState(state: SyncState): Promise<ResultEnvelope<boolean>>

  // Stats
  getWritingStats(): Promise<ResultEnvelope<WritingStats>>
  calculateWordCount(text: string): Promise<number>
  processWritingEvent(deviceId: string, platform: string, projectId: string, volumeId: string,
    chapterId: string, oldText: string, newText: string, durationSeconds: number, sessionId: string): Promise<ResultEnvelope<boolean>>
  getRecentEdits(): Promise<ResultEnvelope<RecentEdit[]>>

  // Layout Contract（#610：输入为平台已判断好的窗口能力）
  resolveLayout(capabilities: WindowCapabilities): Promise<ResultEnvelope<LayoutContract>>

  // Screen Contract（#610：动作区域/顺序是产品语义，不随壳层变化）
  resolveScreenPolicy(screenRole: ScreenRole): Promise<ResultEnvelope<ScreenPolicy>>

  // Native Status
  getNativeLoadStatus(): Promise<ResultEnvelope<string>>
  isAiAvailable(): Promise<ResultEnvelope<boolean>>

  // Editor Animation
  editorVisualTransaction(
    oldText: string,
    newText: string,
    oldCursorIndex: number,
    newCursorIndex: number,
    cause: EditorTransactionCause,
    maxAnimatedChars: number,
    animationDurationMs: number
  ): Promise<ResultEnvelope<EditorVisualTransaction | null>>
}
